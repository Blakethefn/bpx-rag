---
language: swift
---
// Save a display preference in ordinary application defaults.
func saveTheme(_ theme: String) {
    UserDefaults.standard.set(theme, forKey: "theme")
}
