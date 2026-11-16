//! The texts of the plugin, in English and French.

/// The text of `key` in the language of the editor.
pub fn t(french: bool, key: &str) -> &'static str {
    let (en, fr) = match key {
        "intro" => (
            "Each language uses the linters it finds ready in the project. Turn Automatic off to choose them, and give the path of a program that is not found by itself.",
            "Chaque langage utilise les linters qu'il trouve prêts dans le projet. Désactivez Automatique pour les choisir, et indiquez le chemin d'un programme qui n'est pas trouvé tout seul.",
        ),
        "automatic" => ("Automatic", "Automatique"),
        "language_server" => ("Language server", "Serveur de langage"),
        "ready" => ("ready", "prêt"),
        "no_config" => (
            "installed, no configuration in the project",
            "installé, aucune configuration dans le projet",
        ),
        "missing" => ("not installed", "non installé"),
        "path" => (
            "Path of the program (found by itself if empty)",
            "Chemin du programme (trouvé tout seul si vide)",
        ),
        "saved" => ("Lint settings saved", "Réglages du lint enregistrés"),
        "not_saved" => (
            "The lint settings could not be saved",
            "Les réglages du lint n'ont pas pu être enregistrés",
        ),
        _ => ("", ""),
    };
    if french {
        fr
    } else {
        en
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages() {
        for key in [
            "intro",
            "automatic",
            "language_server",
            "ready",
            "no_config",
            "missing",
            "path",
            "saved",
            "not_saved",
        ] {
            assert!(
                !t(false, key).is_empty() && !t(true, key).is_empty(),
                "{key}"
            );
        }
    }
}
