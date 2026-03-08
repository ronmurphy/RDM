/// Generate a GtkSourceView 4 colour scheme XML file derived from the active
/// RDM theme palette and write it to ~/.local/share/gtksourceview-4/styles/.
///
/// This is called once at startup (before GTK initialises) so the scheme is
/// immediately available to StyleSchemeManager when the first view is created.
pub fn generate_rdm_scheme() {
    let colors = rdm_common::theme::load_theme_colors(
        &rdm_common::config::RdmConfig::load().appearance.theme,
    );

    // Build a quick colour lookup.
    let mut map = std::collections::HashMap::new();
    for c in &colors {
        map.insert(c.var_name.as_str(), c.value.as_str());
    }

    let bg      = map.get("theme_bg").copied().unwrap_or("#1e1e2e");
    let fg      = map.get("theme_fg").copied().unwrap_or("#cdd6f4");
    let surface = map.get("theme_surface").copied().unwrap_or("#313244");
    let muted   = map.get("theme_muted").copied().unwrap_or("#6c7086");
    let accent  = map.get("theme_accent").copied().unwrap_or("#89b4fa");
    let green   = map.get("theme_green").copied().unwrap_or("#a6e3a1");
    let yellow  = map.get("theme_yellow").copied().unwrap_or("#f9e2af");
    let red     = map.get("theme_red").copied().unwrap_or("#f38ba8");
    let cyan    = map.get("theme_cyan").copied().unwrap_or("#89dceb");
    let purple  = map.get("theme_purple").copied().unwrap_or("#cba6f7");
    let subtle  = map.get("theme_subtle").copied().unwrap_or("#bac2de");

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<style-scheme id="rdm-theme" name="RDM Theme" version="1.0">
  <author>RDM (generated)</author>
  <description>Colour scheme derived from the active RDM theme palette.</description>

  <!-- Palette colours -->
  <color name="bg"      value="{bg}"/>
  <color name="fg"      value="{fg}"/>
  <color name="surface" value="{surface}"/>
  <color name="muted"   value="{muted}"/>
  <color name="accent"  value="{accent}"/>
  <color name="green"   value="{green}"/>
  <color name="yellow"  value="{yellow}"/>
  <color name="red"     value="{red}"/>
  <color name="cyan"    value="{cyan}"/>
  <color name="purple"  value="{purple}"/>
  <color name="subtle"  value="{subtle}"/>

  <!-- Base styles -->
  <style name="text"                  foreground="fg"     background="bg"/>
  <style name="selection"             background="accent" foreground="bg"/>
  <style name="current-line"          background="surface"/>
  <style name="line-numbers"          foreground="muted"  background="bg"/>
  <style name="right-margin"          foreground="surface"/>
  <style name="bracket-match"         foreground="cyan"   bold="true"/>
  <style name="bracket-mismatch"      foreground="red"    bold="true"/>
  <style name="search-match"          background="yellow" foreground="bg"/>

  <!-- Syntax token styles -->
  <style name="def:comment"           foreground="muted"  italic="true"/>
  <style name="def:doc-comment"       foreground="muted"  italic="true"/>
  <style name="def:string"            foreground="green"/>
  <style name="def:special-char"      foreground="cyan"/>
  <style name="def:keyword"           foreground="accent" bold="true"/>
  <style name="def:builtin"           foreground="cyan"/>
  <style name="def:type"              foreground="purple"/>
  <style name="def:class"             foreground="purple"/>
  <style name="def:function"          foreground="accent"/>
  <style name="def:constant"          foreground="yellow"/>
  <style name="def:number"            foreground="yellow"/>
  <style name="def:base-n-integer"    foreground="yellow"/>
  <style name="def:floating-point"    foreground="yellow"/>
  <style name="def:boolean"           foreground="yellow"/>
  <style name="def:preprocessor"      foreground="cyan"   italic="true"/>
  <style name="def:error"             foreground="red"    underline="true"/>
  <style name="def:warning"           foreground="yellow" underline="true"/>
  <style name="def:identifier"        foreground="fg"/>
  <style name="def:operator"          foreground="subtle"/>
  <style name="def:punctuation"       foreground="subtle"/>
  <style name="def:variable"          foreground="fg"/>

  <!-- Language-specific -->
  <style name="python:decorator"      foreground="cyan"   italic="true"/>
  <style name="python:f-string"       foreground="green"/>
  <style name="rust:lifetime"         foreground="yellow" italic="true"/>
  <style name="rust:macro"            foreground="cyan"/>
  <style name="rust:attribute"        foreground="muted"  italic="true"/>
  <style name="html:tag"              foreground="accent"/>
  <style name="html:attribute-name"   foreground="yellow"/>
  <style name="html:attribute-value"  foreground="green"/>
  <style name="css:property-name"     foreground="cyan"/>
  <style name="css:property-value"    foreground="green"/>
  <style name="css:selector"          foreground="accent"/>
  <style name="js:this"               foreground="red"/>
  <style name="js:arrow"              foreground="accent"/>
</style-scheme>
"#
    );

    // Write to ~/.local/share/gtksourceview-5/styles/rdm-theme.xml
    if let Some(data_dir) = dirs::data_local_dir() {
        let styles_dir = data_dir.join("gtksourceview-5").join("styles");
        if let Err(e) = std::fs::create_dir_all(&styles_dir) {
            log::warn!("Could not create sourceview styles dir: {}", e);
            return;
        }
        let out_path = styles_dir.join("rdm-theme.xml");
        if let Err(e) = std::fs::write(&out_path, &xml) {
            log::warn!("Could not write rdm-theme.xml: {}", e);
        } else {
            log::info!("Wrote GtkSourceView scheme → {}", out_path.display());
        }
    }
}
