use schemars::JsonSchema;
use serde::Serialize;

#[derive(Serialize, JsonSchema)]
pub struct List {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Serialize, JsonSchema)]
pub struct Item {
    pub value: String,
}
