//! Shared helpers used by multiple handler modules.

pub(crate) fn setting_bool(state: &crate::state::WebState, path: &str) -> Option<bool> {
    let map = state.engine().app_state().settings.settings_map();
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = map.get(first)?;
    for part in parts {
        value = value.get(part)?;
    }
    value.as_bool()
}
