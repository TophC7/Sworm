import { backend } from '$lib/api/backend'
import { basename } from '$lib/utils/paths'
import { getBuiltinRuntimeLanguages, preloadBuiltinCatalog } from '$lib/features/builtins/catalog'
import { isFormatterManagedLanguage } from '$lib/features/editor/formatters/config'
import { filePathToLanguage, isBinaryFile } from '$lib/features/editor/languageMap'
import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
import type { LspDocumentSelector, LspEvent, LspServerSettingsEntry } from '$lib/types/backend'

const LSP_MARKER_OWNER = 'sworm-lsp'
const TEXT_DOCUMENT_SYNC_FULL = 1
const TEXT_DOCUMENT_SYNC_INCREMENTAL = 2
const LSP_INSTANCE_IDLE_STOP_MS = 30_000
let monacoRef: Monaco | null = null

type Monaco = typeof import('monaco-editor')
type MonacoModel = import('monaco-editor').editor.ITextModel
type MonacoPosition = import('monaco-editor').Position
type MonacoHover = import('monaco-editor').languages.Hover
type MonacoLocation = import('monaco-editor').languages.Location
type MonacoCompletionList = import('monaco-editor').languages.CompletionList
type MonacoTextEdit = import('monaco-editor').languages.TextEdit
type MonacoDisposable = import('monaco-editor').IDisposable
type MonacoContentChangeEvent = import('monaco-editor').editor.IModelContentChangedEvent
type MonacoSelectionOrPosition = import('monaco-editor').IRange | import('monaco-editor').IPosition

type JsonRpcId = number

interface DocumentContext {
  projectId: string
  projectPath: string
}

interface ManagedDocument {
  model: MonacoModel
  context: DocumentContext
  version: number
  attachments: Set<string>
  openBy: Set<string>
  diagnosticsByServer: Map<string, unknown[]>
  changeDisposable: MonacoDisposable
}

interface PendingRequest {
  resolve: (value: unknown) => void
  reject: (error: unknown) => void
  method: string
}

interface ServerInstance {
  key: string
  sessionId: string
  projectId: string
  rootPath: string
  order: number
  entry: LspServerSettingsEntry
  status: 'starting' | 'ready' | 'failed'
  syncKind: number
  capabilities: Record<string, unknown> | null
  pending: Map<JsonRpcId, PendingRequest>
  nextRequestId: number
  documents: Set<string>
  startPromise: Promise<void> | null
  settings: unknown
  idleStopTimer: ReturnType<typeof setTimeout> | null
}

class LspRegistry {
  private monaco: Monaco | null = null
  private providerInitPromise: Promise<void> | null = null
  private registeredLanguages = new Set<string>()
  private documents = new Map<string, ManagedDocument>()
  private serverInstances = new Map<string, ServerInstance>()
  private serverEntriesByProject = new Map<string, Promise<LspServerSettingsEntry[]>>()
  private knownProjects = new Map<string, string>()
  private editorOpenerRegistered = false

  async ensureMonaco(monaco: Monaco): Promise<void> {
    this.monaco = monaco
    monacoRef = monaco
    if (this.providerInitPromise) {
      await this.providerInitPromise
      return
    }

    this.providerInitPromise = (async () => {
      await preloadBuiltinCatalog()
      this.registerBuiltinLanguages(monaco)
      this.registerProviders(monaco)
    })().catch((error) => {
      this.providerInitPromise = null
      throw error
    })

    await this.providerInitPromise
  }

  async attachModel(model: MonacoModel, context: DocumentContext): Promise<void> {
    if (!this.monaco) return

    const uri = model.uri.toString()
    this.detachModel(model)
    this.knownProjects.set(context.projectId, context.projectPath)

    const document: ManagedDocument = {
      model,
      context,
      version: 1,
      attachments: new Set(),
      openBy: new Set(),
      diagnosticsByServer: new Map(),
      changeDisposable: model.onDidChangeContent((event) => {
        void this.handleDocumentChange(uri, event)
      })
    }

    this.documents.set(uri, document)

    try {
      await this.attachMatchingServers(document)
    } catch (error) {
      console.error('Failed to attach LSP model', error)
    }
  }

  detachModel(model: MonacoModel) {
    const uri = model.uri.toString()
    const document = this.documents.get(uri)
    if (!document) return

    document.changeDisposable.dispose()

    for (const key of document.attachments) {
      const instance = this.serverInstances.get(key)
      if (!instance) continue
      instance.documents.delete(uri)
      void this.closeDocument(instance, document)
      if (instance.documents.size === 0) this.scheduleIdleStop(instance)
    }

    this.documents.delete(uri)
    this.clearMarkers(document.model)
  }

  invalidateServerEntries(projectId?: string) {
    if (projectId) {
      this.serverEntriesByProject.delete(projectId)
      return
    }
    this.serverEntriesByProject.clear()
  }

  async refreshProject(projectId: string): Promise<void> {
    this.invalidateServerEntries(projectId)

    const instances = [...this.serverInstances.values()].filter((instance) => instance.projectId === projectId)
    await Promise.all(instances.map((instance) => this.stopInstance(instance)))

    const documents = [...this.documents.values()].filter((document) => document.context.projectId === projectId)
    await Promise.all(documents.map((document) => this.attachMatchingServers(document)))
  }

  async restartServerDefinition(serverDefinitionId: string): Promise<void> {
    const matchingInstances = [...this.serverInstances.values()].filter(
      (instance) => instance.entry.server.server_definition_id === serverDefinitionId
    )

    await Promise.all(matchingInstances.map((instance) => this.stopInstance(instance)))

    await Promise.all(
      [...this.documents.values()].map((document) => this.attachServerDefinition(document, serverDefinitionId))
    )
  }

  async provideHover(model: MonacoModel, position: MonacoPosition): Promise<MonacoHover | null> {
    const document = await this.getReadyDocument(model)
    if (!document) return null

    const params = {
      textDocument: { uri: model.uri.toString() },
      position: toLspPosition(position)
    }

    const responses = await this.requestAll(document, 'textDocument/hover', params, (instance) =>
      Boolean((instance.capabilities ?? {}).hoverProvider)
    )

    const contents: import('monaco-editor').IMarkdownString[] = []
    let hoverRange: import('monaco-editor').IRange | undefined

    for (const response of responses) {
      if (!response || typeof response !== 'object') continue
      const hover = response as { contents?: unknown; range?: unknown }
      if (!hoverRange && hover.range) {
        hoverRange = fromLspRange(hover.range)
      }
      contents.push(...toMarkdownContents(hover.contents))
    }

    return contents.length > 0 ? { contents, range: hoverRange } : null
  }

  async provideDefinition(model: MonacoModel, position: MonacoPosition): Promise<MonacoLocation[] | null> {
    const document = await this.getReadyDocument(model)
    if (!document) return null

    const params = {
      textDocument: { uri: model.uri.toString() },
      position: toLspPosition(position)
    }

    const response = await this.requestPrimary(document, 'textDocument/definition', params, (instance) =>
      Boolean((instance.capabilities ?? {}).definitionProvider)
    )

    return this.materializeLocations(document, toMonacoLocations(response))
  }

  async provideDocumentFormattingEdits(model: MonacoModel): Promise<MonacoTextEdit[]> {
    const document = await this.getReadyDocument(model)
    if (!document) return []

    const response = await this.requestPrimary(
      document,
      'textDocument/formatting',
      {
        textDocument: { uri: model.uri.toString() },
        options: {
          tabSize: model.getOptions().tabSize,
          insertSpaces: model.getOptions().insertSpaces
        }
      },
      (instance) => Boolean((instance.capabilities ?? {}).documentFormattingProvider)
    )

    return Array.isArray(response) ? response.map(toMonacoTextEdit).filter(isPresent) : []
  }

  async provideCompletionItems(model: MonacoModel, position: MonacoPosition): Promise<MonacoCompletionList> {
    const document = await this.getReadyDocument(model)
    if (!document) return { suggestions: [] }

    const params = {
      textDocument: { uri: model.uri.toString() },
      position: toLspPosition(position)
    }

    const responses = await this.requestAll(document, 'textDocument/completion', params, (instance) =>
      Boolean((instance.capabilities ?? {}).completionProvider)
    )

    const suggestions = responses.flatMap((response) =>
      completionItemsFromResponse(response).map((item) => toMonacoCompletionItem(item, model, position))
    )

    return { suggestions }
  }

  private registerBuiltinLanguages(monaco: Monaco) {
    const existing = new Set(monaco.languages.getLanguages().map((entry) => entry.id))
    for (const language of getBuiltinRuntimeLanguages()) {
      if (existing.has(language.id)) continue
      monaco.languages.register({ id: language.id, extensions: language.extensions, aliases: [language.label] })
      existing.add(language.id)
    }
  }

  private registerProviders(monaco: Monaco) {
    this.registerEditorOpener(monaco)

    for (const languageId of getBuiltinRuntimeLanguages().map((language) => language.id)) {
      if (this.registeredLanguages.has(languageId)) continue
      this.registeredLanguages.add(languageId)

      monaco.languages.registerHoverProvider(languageId, {
        provideHover: (model, position) => this.provideHover(model, position)
      })

      monaco.languages.registerDefinitionProvider(languageId, {
        provideDefinition: (model, position) => this.provideDefinition(model, position)
      })

      monaco.languages.registerCompletionItemProvider(languageId, {
        triggerCharacters: ['.', ':', '"', "'", '/', '-', '<', ' '],
        provideCompletionItems: (model, position) => this.provideCompletionItems(model, position)
      })

      if (!isFormatterManagedLanguage(languageId)) {
        monaco.languages.registerDocumentFormattingEditProvider(languageId, {
          provideDocumentFormattingEdits: (model) => this.provideDocumentFormattingEdits(model)
        })
      }
    }
  }

  getDocumentContext(model: MonacoModel): DocumentContext | null {
    return this.documents.get(model.uri.toString())?.context ?? null
  }

  private registerEditorOpener(monaco: Monaco) {
    if (this.editorOpenerRegistered) return

    monaco.editor.registerEditorOpener({
      openCodeEditor: async (source, resource, selectionOrPosition) => {
        const sourceModel = source.getModel()
        if (!sourceModel) return false

        if (sourceModel.uri.toString() === resource.toString()) {
          applyEditorSelection(source, selectionOrPosition)
          source.focus()
          return true
        }

        const targetPath = fileUriToPath(resource)
        if (!targetPath || isBinaryFile(targetPath)) return false

        const context = this.resolveTargetContext(sourceModel.uri.toString(), targetPath)
        if (!context) return false

        const relativePath = relativePathFromRoot(context.projectPath, targetPath)
        if (!relativePath) return false

        const modelReady = await this.ensureFileModel(resource, context)
        if (!modelReady) return false

        await openTextFile(context.projectId, relativePath, {
          temporary: true,
          reveal: selectionOrPosition ? toEditorRevealTarget(selectionOrPosition) : null
        })
        return true
      }
    })

    this.editorOpenerRegistered = true
  }

  private async getProjectServerEntries(projectId: string): Promise<LspServerSettingsEntry[]> {
    const cached = this.serverEntriesByProject.get(projectId)
    if (cached) return cached

    const promise = backend.lsp.listServers(projectId).catch((error) => {
      this.serverEntriesByProject.delete(projectId)
      throw error
    })
    this.serverEntriesByProject.set(projectId, promise)
    return promise
  }

  private ensureServerInstance(context: DocumentContext, entry: LspServerSettingsEntry, order: number): ServerInstance {
    const rootPath = context.projectPath
    const key = `${context.projectId}:${entry.server.server_definition_id}:${rootPath}`
    const existing = this.serverInstances.get(key)
    if (existing) return existing

    const settings = safeJsonParse(entry.config.settings_json)
    const instance: ServerInstance = {
      key,
      sessionId: key,
      projectId: context.projectId,
      rootPath,
      order,
      entry,
      status: 'starting',
      syncKind: TEXT_DOCUMENT_SYNC_FULL,
      capabilities: null,
      pending: new Map(),
      nextRequestId: 1,
      documents: new Set(),
      startPromise: null,
      settings,
      idleStopTimer: null
    }

    instance.startPromise = this.startServer(instance)
    this.serverInstances.set(key, instance)
    return instance
  }

  private async startServer(instance: ServerInstance): Promise<void> {
    try {
      await backend.lsp.start(
        instance.sessionId,
        instance.projectId,
        instance.entry.server.server_definition_id,
        instance.rootPath,
        (event) => this.onServerEvent(instance.key, event)
      )

      const initializeResult = (await this.request(instance, 'initialize', {
        processId: null,
        clientInfo: { name: 'Sworm' },
        rootUri: modelPathToFileUri(instance.rootPath),
        rootPath: instance.rootPath,
        initializationOptions: instance.entry.server.initialization_options ?? undefined,
        capabilities: clientCapabilities(),
        workspaceFolders: [
          {
            uri: modelPathToFileUri(instance.rootPath),
            name: basename(instance.rootPath)
          }
        ]
      })) as { capabilities?: Record<string, unknown> } | null

      instance.capabilities = initializeResult?.capabilities ?? {}
      instance.syncKind = getTextDocumentSyncKind(instance.capabilities?.textDocumentSync)
      instance.status = 'ready'

      await this.notify(instance, 'initialized', {})
      if (instance.settings != null) {
        await this.notify(instance, 'workspace/didChangeConfiguration', {
          settings: instance.settings
        })
      }

      for (const uri of instance.documents) {
        const document = this.documents.get(uri)
        if (document) {
          await this.openDocument(instance, document)
        }
      }
    } catch (error) {
      instance.status = 'failed'
      this.rejectPending(instance, error)
      console.error(`Failed to start LSP server ${instance.entry.server.label}`, error)
    }
  }

  private async stopInstance(instance: ServerInstance): Promise<void> {
    this.cancelIdleStop(instance)
    this.serverInstances.delete(instance.key)
    this.rejectPending(instance, new Error(`LSP server stopped: ${instance.key}`))
    for (const uri of instance.documents) {
      const document = this.documents.get(uri)
      if (!document) continue
      document.attachments.delete(instance.key)
      document.openBy.delete(instance.key)
      document.diagnosticsByServer.delete(instance.key)
      this.applyDiagnostics(document)
    }
    try {
      await backend.lsp.stop(instance.sessionId)
    } catch (error) {
      console.warn(`Failed to stop LSP server ${instance.sessionId}`, error)
    }
  }

  private async attachServerDefinition(document: ManagedDocument, serverDefinitionId: string): Promise<void> {
    await this.attachMatchingServers(document, (entry) => entry.server.server_definition_id === serverDefinitionId)
  }

  private async attachMatchingServers(
    document: ManagedDocument,
    entryFilter: (entry: LspServerSettingsEntry) => boolean = () => true
  ): Promise<void> {
    const entries = await this.getProjectServerEntries(document.context.projectId)
    const matches = entries
      .map((entry, index) => ({ entry, index }))
      .filter(({ entry }) => entryFilter(entry))
      .filter(({ entry }) => entry.config.enabled)
      .filter(({ entry }) => matchesSelectors(entry.server.document_selectors, document.model))

    const instances: ServerInstance[] = []
    for (const { entry, index } of matches) {
      const instance = this.ensureServerInstance(document.context, entry, index)
      this.cancelIdleStop(instance)
      document.attachments.add(instance.key)
      instance.documents.add(document.model.uri.toString())
      instances.push(instance)
    }

    await Promise.all(instances.map((instance) => instance.startPromise).filter(Boolean))

    await Promise.all(
      instances.map(async (instance) => {
        if (instance.status !== 'ready') return
        await this.openDocument(instance, document)
      })
    )
  }

  private async getReadyDocument(model: MonacoModel): Promise<ManagedDocument | null> {
    const document = this.documents.get(model.uri.toString())
    if (!document) return null

    await Promise.all(
      [...document.attachments].map((key) => this.serverInstances.get(key)?.startPromise ?? null).filter(Boolean)
    )

    return document
  }

  private async materializeLocations(
    document: ManagedDocument,
    locations: MonacoLocation[] | null
  ): Promise<MonacoLocation[] | null> {
    if (!locations || locations.length === 0) return null

    const resolved = await Promise.all(
      locations.map(async (location) => {
        if (this.monaco?.editor.getModel(location.uri)) return location

        const targetPath = fileUriToPath(location.uri)
        if (!targetPath || isBinaryFile(targetPath)) return null

        const context = this.resolveTargetContext(document.model.uri.toString(), targetPath)
        if (!context) return null

        return (await this.ensureFileModel(location.uri, context)) ? location : null
      })
    )

    const filtered = resolved.filter(isPresent)
    return filtered.length > 0 ? filtered : null
  }

  private resolveTargetContext(sourceUri: string, targetPath: string): DocumentContext | null {
    const sourceContext = this.documents.get(sourceUri)?.context
    if (sourceContext && isPathWithinRoot(targetPath, sourceContext.projectPath)) {
      return sourceContext
    }

    const candidates = [...this.knownProjects.entries()]
      .map(([projectId, projectPath]) => ({ projectId, projectPath }))
      .filter((context) => isPathWithinRoot(targetPath, context.projectPath))
      .sort((a, b) => b.projectPath.length - a.projectPath.length)

    return candidates[0] ?? null
  }

  private async ensureFileModel(resource: import('monaco-editor').Uri, context: DocumentContext): Promise<boolean> {
    if (!this.monaco) return false
    if (this.monaco.editor.getModel(resource)) return true

    const targetPath = fileUriToPath(resource)
    if (!targetPath || isBinaryFile(targetPath)) return false

    const relativePath = relativePathFromRoot(context.projectPath, targetPath)
    if (!relativePath) return false

    try {
      const content = await backend.files.read(context.projectPath, relativePath)
      this.monaco.editor.createModel(content, filePathToLanguage(targetPath), resource)
      return true
    } catch (error) {
      console.warn(`Failed to preload LSP target model ${targetPath}`, error)
      return false
    }
  }

  private scheduleIdleStop(instance: ServerInstance) {
    if (instance.idleStopTimer || instance.documents.size > 0) return

    instance.idleStopTimer = setTimeout(() => {
      instance.idleStopTimer = null
      if (instance.documents.size > 0) return
      void this.stopInstance(instance)
    }, LSP_INSTANCE_IDLE_STOP_MS)
  }

  private cancelIdleStop(instance: ServerInstance) {
    if (!instance.idleStopTimer) return
    clearTimeout(instance.idleStopTimer)
    instance.idleStopTimer = null
  }

  private async handleDocumentChange(uri: string, event: MonacoContentChangeEvent) {
    const document = this.documents.get(uri)
    if (!document) return

    document.version += 1

    await Promise.all(
      [...document.attachments].map(async (key) => {
        const instance = this.serverInstances.get(key)
        if (!instance || instance.status !== 'ready' || !document.openBy.has(key)) return

        const contentChanges =
          instance.syncKind === TEXT_DOCUMENT_SYNC_INCREMENTAL
            ? event.changes.map((change) => ({
                range: toLspRange(change.range),
                rangeLength: change.rangeLength,
                text: change.text
              }))
            : [{ text: document.model.getValue() }]

        await this.notify(instance, 'textDocument/didChange', {
          textDocument: { uri, version: document.version },
          contentChanges
        })
      })
    )
  }

  private async openDocument(instance: ServerInstance, document: ManagedDocument) {
    if (instance.status !== 'ready' || document.openBy.has(instance.key)) return
    document.openBy.add(instance.key)
    await this.notify(instance, 'textDocument/didOpen', {
      textDocument: {
        uri: document.model.uri.toString(),
        languageId: document.model.getLanguageId(),
        version: document.version,
        text: document.model.getValue()
      }
    })
  }

  private async closeDocument(instance: ServerInstance, document: ManagedDocument) {
    if (instance.status !== 'ready' || !document.openBy.has(instance.key)) return
    document.openBy.delete(instance.key)
    await this.notify(instance, 'textDocument/didClose', {
      textDocument: { uri: document.model.uri.toString() }
    })
  }

  private async request(instance: ServerInstance, method: string, params: unknown): Promise<unknown> {
    const id = instance.nextRequestId++
    return new Promise((resolve, reject) => {
      instance.pending.set(id, { resolve, reject, method })
      backend.lsp
        .send(
          instance.sessionId,
          JSON.stringify({
            jsonrpc: '2.0',
            id,
            method,
            params
          })
        )
        .catch((error) => {
          instance.pending.delete(id)
          reject(error)
        })
    })
  }

  private async notify(instance: ServerInstance, method: string, params: unknown): Promise<void> {
    await backend.lsp.send(
      instance.sessionId,
      JSON.stringify({
        jsonrpc: '2.0',
        method,
        params
      })
    )
  }

  private onServerEvent(instanceKey: string, event: LspEvent) {
    const instance = this.serverInstances.get(instanceKey)
    if (!instance) return

    switch (event.type) {
      case 'message':
        this.handleServerMessage(instance, event.payload_json)
        break
      case 'trace':
        if (instance.entry.config.trace !== 'off') {
          console.debug(`[lsp:${instance.entry.server.label}]`, event.direction, event.payload)
        }
        break
      case 'error':
        console.warn(`[lsp:${instance.entry.server.label}] ${event.message}`)
        break
      case 'exit':
        void this.stopInstance(instance)
        break
      case 'started':
        break
    }
  }

  private handleServerMessage(instance: ServerInstance, payloadJson: string) {
    let message: Record<string, unknown>
    try {
      message = JSON.parse(payloadJson)
    } catch (error) {
      console.warn('Failed to parse LSP payload', error)
      return
    }

    if ('id' in message && !('method' in message)) {
      const pending = instance.pending.get(message.id as JsonRpcId)
      if (!pending) return
      instance.pending.delete(message.id as JsonRpcId)
      if ('error' in message && message.error) {
        pending.reject(message.error)
      } else {
        pending.resolve(message.result)
      }
      return
    }

    const method = typeof message.method === 'string' ? message.method : null
    if (!method) return

    if ('id' in message) {
      void this.handleServerRequest(instance, message.id as JsonRpcId, method, message.params)
      return
    }

    this.handleServerNotification(instance, method, message.params)
  }

  private async handleServerRequest(instance: ServerInstance, id: JsonRpcId, method: string, params: unknown) {
    if (method === 'workspace/configuration') {
      const items = Array.isArray((params as { items?: unknown[] } | undefined)?.items)
        ? ((params as { items: unknown[] }).items as unknown[])
        : []
      const result = items.map((item) => configurationValueForSection(instance.settings, item))
      await backend.lsp.send(instance.sessionId, JSON.stringify({ jsonrpc: '2.0', id, result }))
      return
    }

    if (method === 'client/registerCapability' || method === 'client/unregisterCapability') {
      await backend.lsp.send(instance.sessionId, JSON.stringify({ jsonrpc: '2.0', id, result: null }))
      return
    }

    await backend.lsp.send(
      instance.sessionId,
      JSON.stringify({
        jsonrpc: '2.0',
        id,
        error: {
          code: -32601,
          message: `Unsupported client method ${method}`
        }
      })
    )
  }

  private handleServerNotification(instance: ServerInstance, method: string, params: unknown) {
    if (method === 'textDocument/publishDiagnostics') {
      const uri = (params as { uri?: string } | undefined)?.uri
      if (!uri) return
      const document = this.documents.get(uri)
      if (!document) return
      const diagnostics = Array.isArray((params as { diagnostics?: unknown[] }).diagnostics)
        ? ((params as { diagnostics: unknown[] }).diagnostics as unknown[])
        : []
      document.diagnosticsByServer.set(instance.key, diagnostics)
      this.applyDiagnostics(document)
      return
    }

    if (method === 'window/logMessage' || method === 'window/showMessage') {
      const message = (params as { message?: string } | undefined)?.message
      if (message) {
        console.info(`[lsp:${instance.entry.server.label}] ${message}`)
      }
    }
  }

  private applyDiagnostics(document: ManagedDocument) {
    if (!this.monaco) return

    const markers = [...document.diagnosticsByServer.values()]
      .flat()
      .map((diagnostic) => toMonacoMarker(diagnostic))
      .filter(isPresent)

    this.monaco.editor.setModelMarkers(document.model, LSP_MARKER_OWNER, markers)
  }

  private clearMarkers(model: MonacoModel) {
    if (!this.monaco) return
    this.monaco.editor.setModelMarkers(model, LSP_MARKER_OWNER, [])
  }

  private rejectPending(instance: ServerInstance, error: unknown) {
    for (const pending of instance.pending.values()) {
      pending.reject(error)
    }
    instance.pending.clear()
  }

  private async requestAll(
    document: ManagedDocument,
    method: string,
    params: unknown,
    capabilityCheck: (instance: ServerInstance) => boolean
  ): Promise<unknown[]> {
    const instances = this.readyInstances(document, capabilityCheck)
    return Promise.all(instances.map((instance) => this.request(instance, method, params).catch(() => null)))
  }

  private async requestPrimary(
    document: ManagedDocument,
    method: string,
    params: unknown,
    capabilityCheck: (instance: ServerInstance) => boolean
  ): Promise<unknown> {
    for (const instance of this.readyInstances(document, capabilityCheck)) {
      try {
        const result = await this.request(instance, method, params)
        if (hasMeaningfulResult(result)) return result
      } catch (error) {
        console.warn(`LSP ${method} failed for ${instance.entry.server.label}`, error)
      }
    }
    return null
  }

  private readyInstances(
    document: ManagedDocument,
    capabilityCheck: (instance: ServerInstance) => boolean
  ): ServerInstance[] {
    return [...document.attachments]
      .map((key) => this.serverInstances.get(key))
      .filter((instance): instance is ServerInstance => instance != null)
      .filter((instance): instance is ServerInstance => instance.status === 'ready')
      .filter((instance) => capabilityCheck(instance))
      .sort((a, b) => a.order - b.order)
  }
}

const registry = new LspRegistry()

export function ensureMonacoLsp(monaco: Monaco) {
  return registry.ensureMonaco(monaco)
}

export function attachLspModel(model: MonacoModel, context: DocumentContext) {
  return registry.attachModel(model, context)
}

export function detachLspModel(model: MonacoModel) {
  registry.detachModel(model)
}

export function invalidateLspServerEntries(projectId?: string) {
  registry.invalidateServerEntries(projectId)
}

export function restartLspServerDefinition(serverDefinitionId: string) {
  return registry.restartServerDefinition(serverDefinitionId)
}

export function refreshLspProjectEnvironment(projectId: string) {
  return registry.refreshProject(projectId)
}

export function formatDocumentWithLsp(model: MonacoModel) {
  return registry.provideDocumentFormattingEdits(model)
}

export function getLspDocumentContext(model: MonacoModel) {
  return registry.getDocumentContext(model)
}

function matchesSelectors(selectors: LspDocumentSelector[], model: MonacoModel): boolean {
  if (selectors.length === 0) return false

  const languageId = model.getLanguageId()
  const fileName = basename(model.uri.path)
  const extension = normalizeExtension(fileName.includes('.') ? fileName.slice(fileName.lastIndexOf('.')) : '')

  return selectors.some((selector) => {
    const languageMatch = selector.language ? selector.language === languageId : false
    const extensionMatch = extension
      ? selector.extensions.some((value) => normalizeExtension(value) === extension)
      : false
    const fileMatch = selector.filenames.some((value) => value === fileName)
    return languageMatch || extensionMatch || fileMatch
  })
}

function normalizeExtension(value: string): string {
  const trimmed = value.trim().toLowerCase()
  if (!trimmed) return ''
  return trimmed.startsWith('.') ? trimmed : `.${trimmed}`
}

function isPathWithinRoot(path: string, rootPath: string): boolean {
  return path === rootPath || path.startsWith(`${rootPath}/`)
}

function relativePathFromRoot(rootPath: string, path: string): string | null {
  if (!isPathWithinRoot(path, rootPath) || path === rootPath) return null
  return path.slice(rootPath.length).replace(/^\/+/, '')
}

function toLspPosition(position: MonacoPosition) {
  return {
    line: position.lineNumber - 1,
    character: position.column - 1
  }
}

function toLspRange(range: import('monaco-editor').IRange) {
  return {
    start: {
      line: range.startLineNumber - 1,
      character: range.startColumn - 1
    },
    end: {
      line: range.endLineNumber - 1,
      character: range.endColumn - 1
    }
  }
}

function fromLspRange(value: unknown): import('monaco-editor').IRange | undefined {
  const range = value as {
    start?: { line?: number; character?: number }
    end?: { line?: number; character?: number }
  }
  if (
    typeof range?.start?.line !== 'number' ||
    typeof range?.start?.character !== 'number' ||
    typeof range?.end?.line !== 'number' ||
    typeof range?.end?.character !== 'number'
  ) {
    return undefined
  }

  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1
  }
}

function clientCapabilities() {
  return {
    textDocument: {
      synchronization: {
        didSave: false,
        willSave: false,
        willSaveWaitUntil: false
      },
      hover: {
        contentFormat: ['markdown', 'plaintext']
      },
      completion: {
        completionItem: {
          snippetSupport: true,
          documentationFormat: ['markdown', 'plaintext'],
          insertReplaceSupport: true,
          labelDetailsSupport: true
        }
      },
      definition: {
        linkSupport: true
      },
      formatting: {}
    },
    workspace: {
      configuration: true,
      workspaceFolders: true
    }
  }
}

function getTextDocumentSyncKind(sync: unknown): number {
  if (typeof sync === 'number') return sync
  if (sync && typeof sync === 'object' && typeof (sync as { change?: unknown }).change === 'number') {
    return (sync as { change: number }).change
  }
  return TEXT_DOCUMENT_SYNC_FULL
}

function modelPathToFileUri(path: string): string {
  return `file://${encodeURI(path).replace(/#/g, '%23')}`
}

function fileUriToPath(uri: import('monaco-editor').Uri): string | null {
  return uri.scheme === 'file' ? uri.fsPath : null
}

function isRangeSelection(value: MonacoSelectionOrPosition): value is import('monaco-editor').IRange {
  return 'endLineNumber' in value && 'endColumn' in value
}

function applyEditorSelection(
  editor: import('monaco-editor').editor.ICodeEditor,
  selectionOrPosition?: MonacoSelectionOrPosition
) {
  if (!selectionOrPosition) return

  if (isRangeSelection(selectionOrPosition)) {
    editor.setSelection(selectionOrPosition)
    editor.revealRangeInCenter(selectionOrPosition)
    return
  }

  editor.setPosition(selectionOrPosition)
  editor.revealPositionInCenter(selectionOrPosition)
}

function toEditorRevealTarget(selectionOrPosition: MonacoSelectionOrPosition) {
  if (isRangeSelection(selectionOrPosition)) {
    return {
      kind: 'range' as const,
      startLineNumber: selectionOrPosition.startLineNumber,
      startColumn: selectionOrPosition.startColumn,
      endLineNumber: selectionOrPosition.endLineNumber,
      endColumn: selectionOrPosition.endColumn
    }
  }

  return {
    kind: 'position' as const,
    lineNumber: selectionOrPosition.lineNumber,
    column: selectionOrPosition.column
  }
}

function toMarkdownContents(contents: unknown): import('monaco-editor').IMarkdownString[] {
  if (!contents) return []
  if (typeof contents === 'string') {
    return [{ value: contents }]
  }
  if (Array.isArray(contents)) {
    return contents.flatMap((entry) => toMarkdownContents(entry))
  }
  if (typeof contents === 'object') {
    if (typeof (contents as { value?: unknown }).value === 'string') {
      return [{ value: (contents as { value: string }).value }]
    }
    if (
      typeof (contents as { language?: unknown }).language === 'string' &&
      typeof (contents as { value?: unknown }).value === 'string'
    ) {
      return [
        {
          value: `\`\`\`${(contents as { language: string }).language}\n${(contents as { value: string }).value}\n\`\`\``
        }
      ]
    }
  }
  return []
}

function completionItemsFromResponse(response: unknown): Record<string, unknown>[] {
  if (Array.isArray(response)) return response as Record<string, unknown>[]
  if (response && typeof response === 'object' && Array.isArray((response as { items?: unknown[] }).items)) {
    return (response as { items: Record<string, unknown>[] }).items
  }
  return []
}

function toMonacoCompletionItem(
  item: Record<string, unknown>,
  model: MonacoModel,
  position: MonacoPosition
): import('monaco-editor').languages.CompletionItem {
  const word = model.getWordUntilPosition(position)
  const fallbackRange = {
    startLineNumber: position.lineNumber,
    endLineNumber: position.lineNumber,
    startColumn: word.startColumn,
    endColumn: word.endColumn
  }

  const textEdit = item.textEdit as
    | { newText?: string; range?: unknown; insert?: unknown; replace?: unknown }
    | undefined

  const range =
    textEdit?.insert && textEdit?.replace
      ? {
          insert: fromLspRange(textEdit.insert) ?? fallbackRange,
          replace: fromLspRange(textEdit.replace) ?? fallbackRange
        }
      : (fromLspRange(textEdit?.range) ?? fallbackRange)

  return {
    label: typeof item.label === 'string' ? item.label : String(item.label ?? ''),
    kind: lspCompletionKindToMonaco(typeof item.kind === 'number' ? item.kind : 9),
    detail: typeof item.detail === 'string' ? item.detail : undefined,
    documentation:
      typeof item.documentation === 'string'
        ? item.documentation
        : typeof (item.documentation as { value?: unknown } | undefined)?.value === 'string'
          ? ((item.documentation as { value: string }).value ?? undefined)
          : undefined,
    insertText:
      typeof textEdit?.newText === 'string'
        ? textEdit.newText
        : typeof item.insertText === 'string'
          ? item.insertText
          : typeof item.label === 'string'
            ? item.label
            : '',
    insertTextRules: item.insertTextFormat === 2 ? 4 : undefined,
    range,
    sortText: typeof item.sortText === 'string' ? item.sortText : undefined,
    filterText: typeof item.filterText === 'string' ? item.filterText : undefined
  }
}

function lspCompletionKindToMonaco(kind: number): number {
  const map: Record<number, number> = {
    1: 18,
    2: 0,
    3: 1,
    4: 2,
    5: 3,
    6: 4,
    7: 5,
    8: 6,
    9: 7,
    10: 8,
    11: 9,
    12: 10,
    13: 11,
    14: 12,
    15: 13,
    16: 14,
    17: 15,
    18: 16,
    19: 17,
    20: 20,
    21: 21,
    22: 22,
    23: 23,
    24: 24,
    25: 25
  }
  return map[kind] ?? 9
}

function toMonacoLocations(response: unknown): MonacoLocation[] | null {
  const items = Array.isArray(response) ? response : response ? [response] : []
  const locations = items.flatMap((item) => {
    const uri =
      typeof (item as { targetUri?: unknown }).targetUri === 'string'
        ? (item as { targetUri: string }).targetUri
        : typeof (item as { uri?: unknown }).uri === 'string'
          ? (item as { uri: string }).uri
          : null
    const range =
      (item as { targetSelectionRange?: unknown }).targetSelectionRange ?? (item as { range?: unknown }).range
    if (!uri) return []
    const monacoRange = fromLspRange(range)
    if (!monacoRange) return []
    return [{ uri: parseUri(uri), range: monacoRange }]
  })

  return locations.length > 0 ? locations : null
}

function parseUri(uri: string) {
  if (!monacoRef) {
    throw new Error('Monaco is not initialized')
  }
  return monacoRef.Uri.parse(uri)
}

function toMonacoTextEdit(edit: unknown): MonacoTextEdit | null {
  const range = fromLspRange((edit as { range?: unknown } | undefined)?.range)
  const text =
    typeof (edit as { newText?: unknown } | undefined)?.newText === 'string'
      ? ((edit as { newText: string }).newText ?? '')
      : ''
  return range ? { range, text } : null
}

function toMonacoMarker(diagnostic: unknown): import('monaco-editor').editor.IMarkerData | null {
  const range = fromLspRange((diagnostic as { range?: unknown } | undefined)?.range)
  if (!range) return null

  const severityMap: Record<number, number> = {
    1: 8,
    2: 4,
    3: 2,
    4: 1
  }

  return {
    ...range,
    message:
      typeof (diagnostic as { message?: unknown } | undefined)?.message === 'string'
        ? ((diagnostic as { message: string }).message ?? '')
        : 'LSP diagnostic',
    severity: severityMap[Number((diagnostic as { severity?: unknown }).severity)] ?? 4,
    code:
      typeof (diagnostic as { code?: unknown } | undefined)?.code === 'string' ||
      typeof (diagnostic as { code?: unknown } | undefined)?.code === 'number'
        ? String((diagnostic as { code: string | number }).code)
        : undefined,
    source:
      typeof (diagnostic as { source?: unknown } | undefined)?.source === 'string'
        ? ((diagnostic as { source: string }).source ?? undefined)
        : undefined
  }
}

function safeJsonParse(value: string | null): unknown {
  if (!value) return null
  try {
    return JSON.parse(value)
  } catch {
    return null
  }
}

function configurationValueForSection(settings: unknown, item: unknown): unknown {
  const section =
    typeof (item as { section?: unknown } | undefined)?.section === 'string'
      ? ((item as { section: string }).section ?? '')
      : ''

  if (!section.trim()) {
    return settings ?? null
  }

  let current: unknown = settings
  for (const segment of section.split('.').filter(Boolean)) {
    if (current == null) return null

    if (Array.isArray(current)) {
      const index = Number(segment)
      if (!Number.isInteger(index) || index < 0 || index >= current.length) return null
      current = current[index]
      continue
    }

    if (typeof current !== 'object') return null
    if (!Object.prototype.hasOwnProperty.call(current, segment)) return null
    current = (current as Record<string, unknown>)[segment]
  }

  return current ?? null
}

function hasMeaningfulResult(value: unknown): boolean {
  if (value == null) return false
  if (Array.isArray(value)) return value.length > 0
  return true
}

function isPresent<T>(value: T | null | undefined): value is T {
  return value != null
}
