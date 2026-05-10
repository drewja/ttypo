use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "resources/runtime"]
pub struct Resources;
