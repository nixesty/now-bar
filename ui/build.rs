// The settings window, with its English bundled: ../i18n/en/LC_MESSAGES/pebbly-ui.po (msgids are
// the Turkish strings themselves, no context; the Rust side's table comes from the same file).
fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("../i18n")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("main.slint", config).expect("ui/main.slint");
}
