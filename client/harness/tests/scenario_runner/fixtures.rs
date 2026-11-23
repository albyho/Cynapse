use super::*;

pub(super) fn bad_runtime_action_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::json!({
            "id": "bad_runtime_action",
            "title": "Bad Runtime Action",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Chapter",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [{ "step_file": "steps/bad.json" }]
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("steps/bad.json"),
        serde_json::json!({
            "step_id": "bad",
            "title": "Bad runtime action",
            "actions": [
                { "type": "TerminalEnsure", "session_key": "main", "cwd": "." },
                {
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "echo {{resource:missing}}",
                    "accepted_exit_codes": [0]
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
}

pub(super) fn terminal_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::json!({
            "id": "terminal_harness",
            "title": "Terminal Harness",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "terminal",
                "title": "Terminal",
                "children": [{
                    "id": "run",
                    "title": "Run",
                    "steps": [{ "step_file": "steps/run.json" }]
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("steps/run.json"),
        serde_json::json!({
            "step_id": "run",
            "title": "Run a command",
            "actions": [
                {
                    "type": "TerminalEnsure",
                    "session_key": "main",
                    "cwd": "."
                },
                {
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "example --version",
                    "expected_output": ["harness-ok"]
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
}

pub(super) fn insert_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "insert_demo",
            "title": "Insert Demo",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "editing",
                "title": "Editing",
                "children": [{
                    "id": "insert",
                    "title": "Insert",
                    "steps": [{ "step_file": "steps/insert.json" }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("steps/insert.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "insert",
            "title": "Insert code",
            "requires_human": true,
            "actions": [
                {
                    "type": "CreateFile",
                    "path": "main.rs"
                },
                {
                    "type": "OpenFile",
                    "path": "main.rs"
                },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"line": 1},
                    "text": "fn main() {\n",
                    "speed_ms": 1
                },
                {
                    "type": "OpenFile",
                    "path": "main.rs"
                },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"line": 2, "at": 0},
                    "text": "    println!(\"ok\");\n}\n"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
}

pub(super) fn edit_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "edit_demo",
            "title": "Edit Demo",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "editing",
                "title": "Editing",
                "children": [{
                    "id": "edit",
                    "title": "Edit",
                    "steps": [{ "step_file": "steps/edit.json" }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("steps/edit.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "edit",
            "title": "Edit code",
            "requires_human": true,
            "actions": [
                {
                    "type": "CreateFile",
                    "path": "main.rs"
                },
                {
                    "type": "OpenFile",
                    "path": "main.rs"
                },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"line": 1},
                    "text": "fn main() {\n    // temporary\n    println!(\"old\");\n}\n"
                },
                {
                    "type": "DeleteText",
                    "path": "main.rs",
                    "target": {
                        "start": {"at": {"text": "    // temporary\n"}},
                        "end": {"at": {"text": "    // temporary\n"}},
                        "expected_text": "    // temporary\n"
                    },
                    "direction": "delete"
                },
                {
                    "type": "ReplaceText",
                    "path": "main.rs",
                    "target": {
                        "start": {"at": {"text": "old"}},
                        "end": {"at": {"text": "old"}},
                        "expected_text": "old"
                    },
                    "text": "new"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
}

pub(super) fn complete_edit_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "complete_edit_demo",
            "title": "Complete Edit Demo",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "editing",
                "title": "Editing",
                "children": [{
                    "id": "complete",
                    "title": "Complete",
                    "steps": [
                        {"step_file": "steps/write.json"},
                        {"step_file": "steps/fail.json"},
                        {"step_file": "steps/repair.json"},
                        {"step_file": "steps/compile.json"},
                        {"step_file": "steps/run.json"}
                    ]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let steps = [
        (
            "write",
            serde_json::json!([
                {
                    "type": "CreateFile",
                    "path": "main.rs"
                },
                {
                    "type": "OpenFile",
                    "path": "main.rs"
                },
                {
                    "type": "InsertText",
                    "path": "main.rs",
                    "target": {"line": 1},
                    "text": "fn main() {\n    let x = 5;\n    // temporary\n    println!(\"{}\", x);\n    x = 6;\n    println!(\"{}\", x);\n}\n"
                }
            ]),
        ),
        (
            "fail",
            serde_json::json!([
                {
                    "type": "TerminalEnsure",
                    "session_key": "main",
                    "cwd": "."
                },
                {
                    "type": "TerminalCommand",
                    "session_key": "main",
                    "cwd": ".",
                    "command": "rustc main.rs",
                    "accepted_exit_codes": [1],
                    "expected_output": ["cannot assign twice to immutable variable"]
                }
            ]),
        ),
        (
            "repair",
            serde_json::json!([
                {
                    "type": "OpenFile",
                    "path": "main.rs"
                },
                {
                    "type": "DeleteText",
                    "path": "main.rs",
                    "target": {
                        "start": {"at": {"text": "    // temporary\n"}},
                        "end": {"at": {"text": "    // temporary\n"}},
                        "expected_text": "    // temporary\n"
                    },
                    "direction": "delete"
                },
                {
                    "type": "ReplaceText",
                    "path": "main.rs",
                    "target": {
                        "start": {"at": {"text": "let x = 5;"}},
                        "end": {"at": {"text": "let x = 5;"}},
                        "expected_text": "let x = 5;\n"
                    },
                    "text": "let mut x = 5;\n"
                }
            ]),
        ),
        (
            "compile",
            serde_json::json!([{
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": "rustc main.rs",
                "accepted_exit_codes": [0],
                "expected_output": []
            }]),
        ),
        (
            "run",
            serde_json::json!([{
                "type": "TerminalCommand",
                "session_key": "main",
                "cwd": ".",
                "command": ".\\main.exe",
                "accepted_exit_codes": [0],
                "expected_output": ["The value of x is: 5"]
            }]),
        ),
    ];
    for (step_id, actions) in steps {
        fs::write(
            root.join("steps").join(format!("{step_id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "step_id": step_id,
                "title": step_id,
                "actions": actions
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

pub(super) fn continuous_resume_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "continuous_resume",
            "title": "Continuous Resume",
            "workspace": "workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [
                    {
                        "id": "a",
                        "title": "A",
                        "steps": [{"step_file": "steps/a.json"}]
                    },
                    {
                        "id": "b",
                        "title": "B",
                        "steps": [
                            {"step_file": "steps/b_first.json"},
                            {"step_file": "steps/b_second.json"}
                        ]
                    },
                    {
                        "id": "c",
                        "title": "C",
                        "steps": [{"step_file": "steps/c.json"}]
                    },
                    {
                        "id": "d",
                        "title": "D",
                        "steps": [{"step_file": "steps/d.json"}]
                    }
                ]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    for step_id in ["a", "b_first", "b_second", "c", "d"] {
        fs::write(
            root.join("steps").join(format!("{step_id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "step_id": step_id,
                "title": step_id,
                "actions": [{
                    "type": "CreateFile",
                    "path": format!("{step_id}.txt")
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

pub(super) fn human_final_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::json!({
            "id": "human_final",
            "title": "Human Final",
            "workspace": "workspace",
            "children": [{
                "id": "ch",
                "title": "Chapter",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [{ "step_file": "steps/final.json" }]
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("steps/final.json"),
        serde_json::json!({
            "step_id": "final",
            "title": "Final human step",
            "requires_human": true,
            "actions": [{ "type": "Present" }]
        })
        .to_string(),
    )
    .unwrap();
}

pub(super) fn human_final_before_next_course(root: &Path) {
    fs::create_dir_all(root.join("workspace")).unwrap();
    fs::create_dir_all(root.join("steps")).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::json!({
            "id": "human_then_next",
            "title": "Human Then Next",
            "workspace": "workspace",
            "children": [{
                "id": "ch",
                "title": "Chapter",
                "children": [
                    {
                        "id": "unit",
                        "title": "Human Unit",
                        "steps": [{ "step_file": "steps/final.json" }]
                    },
                    {
                        "id": "next",
                        "title": "Next Unit",
                        "steps": [{ "step_file": "steps/next.json" }]
                    }
                ]
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("steps/final.json"),
        serde_json::json!({
            "step_id": "final",
            "title": "Final human step",
            "requires_human": true,
            "actions": [{ "type": "Present" }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("steps/next.json"),
        serde_json::json!({
            "step_id": "next",
            "title": "Next step",
            "actions": [{ "type": "Present" }]
        })
        .to_string(),
    )
    .unwrap();
}
