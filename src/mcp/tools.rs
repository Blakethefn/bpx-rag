use serde_json::{json, Value};

pub fn tool_definitions() -> Value {
    json!({
        "tools": [{
            "name": "bpx_file_relationships",
            "description": "Analyze a file from the current filesystem. Finds outgoing Markdown wikilinks, same-crate Rust module/import references, relative TypeScript/JavaScript imports, and files that directly refer to the selected file. Returns project-relative paths and line evidence. No index, database, or embeddings are used.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to analyze. May be absolute or relative to the project root/current directory."
                    },
                    "root": {
                        "type": "string",
                        "description": "Optional project root to scan. Defaults to the nearest recognized project root."
                    }
                },
                "required": ["path"]
            }
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationships_is_the_only_exposed_mcp_tool() {
        let tools = tool_definitions()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "bpx_file_relationships");
        assert_eq!(tools[0]["inputSchema"]["required"][0], "path");
    }
}
