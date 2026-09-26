-- Recent folders gained an open timestamp. Convert a legacy MRU array of
-- path strings into `{path, opened_at}` objects, keeping MRU order with
-- synthetic timestamps one minute apart (most recent = now). Values that are
-- not a pure string array (malformed, already migrated) stay untouched.
UPDATE app_state
SET value_json = (
  SELECT json_group_array(json_object(
    'path', value,
    'opened_at', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now', '-' || key || ' minutes')
  ))
  FROM (SELECT key, value FROM json_each(app_state.value_json) ORDER BY key)
)
WHERE key = 'recent_folders'
  AND json_valid(value_json)
  AND json_type(value_json) = 'array'
  AND NOT EXISTS (
    SELECT 1 FROM json_each(app_state.value_json) WHERE type <> 'text'
  );
