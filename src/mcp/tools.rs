use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;

use super::handlers;
use crate::app::RagApp;

mod index;
mod operate;

pub fn tool_definitions() -> Value {
    json!({
        "tools": catalog().iter().map(Tool::definition).collect::<Vec<_>>()
    })
}

pub(super) fn call_tool(
    name: &str,
    args: &Value,
    project_dir: &Path,
    app_slot: &mut Option<RagApp>,
) -> Result<String> {
    tool_by_name(name)
        .ok_or_else(|| anyhow!("unknown tool: {name}"))?
        .call(args, project_dir, app_slot)
}

struct Tool {
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &'static [&'static str],
    invoke: Invoke,
}

enum Invoke {
    App(fn(&Value, &mut RagApp) -> Result<String>),
    Relate,
}

impl Tool {
    fn definition(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": {
                "type": "object",
                "properties": self.properties.clone(),
                "required": self.required,
            }
        })
    }

    fn call(
        self,
        args: &Value,
        project_dir: &Path,
        app_slot: &mut Option<RagApp>,
    ) -> Result<String> {
        match self.invoke {
            Invoke::App(handler) => with_app(app_slot, |app| handler(args, app)),
            Invoke::Relate => handlers::handle_file_relationships(args, project_dir),
        }
    }
}

fn with_app(
    slot: &mut Option<RagApp>,
    operation: impl FnOnce(&mut RagApp) -> Result<String>,
) -> Result<String> {
    if slot.is_none() {
        *slot = Some(RagApp::open()?);
    }
    operation(slot.as_mut().expect("RAG app was just initialized"))
}

fn tool_by_name(name: &str) -> Option<Tool> {
    catalog().into_iter().find(|tool| tool.name == name)
}

fn catalog() -> Vec<Tool> {
    vec![
        index::search(),
        index::add(),
        index::sync(),
        index::status(),
        index::remove(),
        index::read(),
        operate::models(),
        operate::embed(),
        operate::symbol(),
        operate::findings(),
        operate::watch(),
        operate::relate(),
    ]
}

#[cfg(test)]
mod tests;
