// Git graph swimlane computation and SVG path rendering.
//
// Algorithm and rendering closely follow VS Code's scmHistory.ts
// (MIT-licensed, microsoft/vscode). Constants, lane logic, and SVG
// path shapes are adapted from that implementation.

import type { GraphCommit } from '$lib/types/backend'

// ── VS Code constants (scmHistory.ts) ──────────────────────────
export const SWIMLANE_HEIGHT = 22
export const SWIMLANE_WIDTH = 11
const SWIMLANE_CURVE_RADIUS = 5
export const CIRCLE_RADIUS = 4

// ── Lane colors (adapted to warm palette) ──────────────────────
export const GRAPH_COLORS = [
  '#ffb59f', // peach (accent)
  '#98ff7f', // green
  '#ffe572', // yellow
  '#ff7672', // red
  '#b66dff' // purple
]

// ── Data model ─────────────────────────────────────────────────

export interface GraphNode {
  id: string
  color: number
}

export interface GraphRow {
  commit: GraphCommit
  inputSwimlanes: GraphNode[]
  outputSwimlanes: GraphNode[]
}

export interface RowPath {
  d: string
  color: string
}

export interface RowCircle {
  cx: number
  cy: number
  r: number
  color: string
  isMerge: boolean
}

export interface RowRender {
  paths: RowPath[]
  circle: RowCircle
  width: number
}

// ── Lane computation ───────────────────────────────────────────
// Mirrors VS Code's toISCMHistoryItemViewModelArray.
//
// For each commit (topo order, newest first):
//   - inputSwimlanes = previous row's outputSwimlanes
//   - Lanes matching the commit id are consumed; the first one
//     continues as the commit's first parent
//   - Additional parents spawn new lanes
//   - All other lanes pass through

export function computeGraph(commits: GraphCommit[]): GraphRow[] {
  let colorIndex = -1
  const rows: GraphRow[] = []

  for (const commit of commits) {
    const prevOutput = rows.length > 0 ? rows[rows.length - 1].outputSwimlanes : []
    const inputSwimlanes: GraphNode[] = prevOutput.map((n) => ({ ...n }))
    const outputSwimlanes: GraphNode[] = []

    let firstParentAdded = false

    // Process existing lanes: pass-through or consume
    for (const node of inputSwimlanes) {
      if (node.id === commit.hash) {
        if (!firstParentAdded && commit.parents.length > 0) {
          outputSwimlanes.push({ id: commit.parents[0], color: node.color })
          firstParentAdded = true
        }
        // Lane consumed by commit (merge convergence if duplicate)
        continue
      }
      outputSwimlanes.push({ ...node })
    }

    // Unprocessed parents get new lanes
    for (let i = firstParentAdded ? 1 : 0; i < commit.parents.length; i++) {
      colorIndex = (colorIndex + 1) % GRAPH_COLORS.length
      outputSwimlanes.push({ id: commit.parents[i], color: colorIndex })
    }

    rows.push({ commit, inputSwimlanes, outputSwimlanes })
  }

  return rows
}

// ── SVG path computation ───────────────────────────────────────
// Mirrors VS Code's renderSCMHistoryItemGraph.

function findLastIndex(nodes: GraphNode[], id: string): number {
  for (let i = nodes.length - 1; i >= 0; i--) {
    if (nodes[i].id === id) return i
  }
  return -1
}

function color(idx: number): string {
  return GRAPH_COLORS[idx % GRAPH_COLORS.length]
}

export function computeRowRender(row: GraphRow): RowRender {
  const { commit, inputSwimlanes, outputSwimlanes } = row
  const paths: RowPath[] = []

  // Circle position: leftmost input lane expecting this commit, or appended
  const inputIndex = inputSwimlanes.findIndex((n) => n.id === commit.hash)
  const circleIndex = inputIndex !== -1 ? inputIndex : inputSwimlanes.length

  // Circle color: prefer output lane at circle position, then input
  const circleColorIdx =
    circleIndex < outputSwimlanes.length
      ? outputSwimlanes[circleIndex].color
      : circleIndex < inputSwimlanes.length
        ? inputSwimlanes[circleIndex].color
        : 0

  // ── Input-lane pass-through / merge / shift ──────────────────
  let outputIdx = 0
  for (let index = 0; index < inputSwimlanes.length; index++) {
    const c = color(inputSwimlanes[index].color)

    if (inputSwimlanes[index].id === commit.hash) {
      // Lane converges to commit
      if (index !== circleIndex) {
        // Merge arc: curve from this column to the circle column
        const x1 = SWIMLANE_WIDTH * (index + 1)
        const xC = SWIMLANE_WIDTH * (circleIndex + 1)
        paths.push({
          d: `M ${x1} 0 A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${x1 - SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} H ${xC}`,
          color: c
        })
      } else {
        // Primary lane — just advance output pointer
        outputIdx++
      }
    } else {
      // Not this commit — pass through or shift left
      if (outputIdx < outputSwimlanes.length && inputSwimlanes[index].id === outputSwimlanes[outputIdx].id) {
        if (index === outputIdx) {
          // Straight vertical
          const x = SWIMLANE_WIDTH * (index + 1)
          paths.push({ d: `M ${x} 0 V ${SWIMLANE_HEIGHT}`, color: c })
        } else {
          // Shift: |, curve, -, curve, |
          const x1 = SWIMLANE_WIDTH * (index + 1)
          const x2 = SWIMLANE_WIDTH * (outputIdx + 1)
          const r = SWIMLANE_CURVE_RADIUS
          const mid = SWIMLANE_HEIGHT / 2
          paths.push({
            d: [
              `M ${x1} 0`,
              `V 6`,
              `A ${r} ${r} 0 0 1 ${x1 - r} ${mid}`,
              `H ${x2 + r}`,
              `A ${r} ${r} 0 0 0 ${x2} ${mid + r}`,
              `V ${SWIMLANE_HEIGHT}`
            ].join(' '),
            color: c
          })
        }
        outputIdx++
      }
    }
  }

  // ── Branch-out lines for additional parents ──────────────────
  for (let i = 1; i < commit.parents.length; i++) {
    const parentIdx = findLastIndex(outputSwimlanes, commit.parents[i])
    if (parentIdx === -1) continue

    const c = color(outputSwimlanes[parentIdx].color)
    const xP = SWIMLANE_WIDTH * parentIdx
    const xC = SWIMLANE_WIDTH * (circleIndex + 1)
    const mid = SWIMLANE_HEIGHT / 2

    // Horizontal to circle, then arc down to new lane
    paths.push({
      d: [
        `M ${xP} ${mid}`,
        `A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${xP + SWIMLANE_WIDTH} ${SWIMLANE_HEIGHT}`,
        `M ${xP} ${mid}`,
        `H ${xC}`
      ].join(' '),
      color: c
    })
  }

  // ── Vertical stem to/from circle ─────────────────────────────
  const cx = SWIMLANE_WIDTH * (circleIndex + 1)

  // Stem from top to circle
  if (inputIndex !== -1) {
    paths.push({
      d: `M ${cx} 0 V ${SWIMLANE_HEIGHT / 2}`,
      color: color(inputSwimlanes[inputIndex].color)
    })
  }

  // Stem from circle to bottom
  if (commit.parents.length > 0) {
    paths.push({
      d: `M ${cx} ${SWIMLANE_HEIGHT / 2} V ${SWIMLANE_HEIGHT}`,
      color: color(circleColorIdx)
    })
  }

  // ── Dimensions and circle ────────────────────────────────────
  const maxCols = Math.max(inputSwimlanes.length, outputSwimlanes.length, 1) + 1
  const isMerge = commit.parents.length > 1

  return {
    paths,
    circle: {
      cx,
      cy: SWIMLANE_WIDTH,
      r: isMerge ? CIRCLE_RADIUS + 2 : CIRCLE_RADIUS + 1,
      color: color(circleColorIdx),
      isMerge
    },
    width: SWIMLANE_WIDTH * maxCols
  }
}
