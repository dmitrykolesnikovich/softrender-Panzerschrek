use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct RendererConfig
{
	#[serde(default)]
	pub clear_background: bool,
}

impl RendererConfig
{
	pub fn from_app_config(app_config: &serde_json::Value) -> Self
	{
		serde_json::from_value(app_config["renderer"].clone()).unwrap_or_default()
	}
}