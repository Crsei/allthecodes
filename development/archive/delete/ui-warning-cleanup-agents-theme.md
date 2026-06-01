# UI Warning Cleanup: Agents/Theme

## 2026-05-20

- Deleted `ThemeProvider` module's test-only `save_theme_setting` wrapper.
  The wrapper wrote to the real user settings path and was unused; tests already
  exercise persistence through `write_theme_setting_to_path` with a temporary
  settings file.
