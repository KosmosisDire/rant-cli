//! The presets of `rant new`: folders of files under templates/, compiled in. `{{key}}` in a
//! file's path or text is the value given for key, and nothing else is filled in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rant_config::paths;

use crate::failure::{Result, failed};

include!(concat!(env!("OUT_DIR"), "/templates.rs"));

/// Files to write, each with its text.
pub type Files = Vec<(PathBuf, String)>;

/// What fills each `{{key}}`: name, Name in PascalCase, and framework for a C# preset.
pub type Values = BTreeMap<&'static str, String>;

/// Each preset and the template folders whose files it writes, in order.
const PRESETS: &[(&str, &[&str])] = &[
    ("node-cpp", &["node-cpp"]),
    ("node-python", &["node-python"]),
    ("node-rust", &["node-rust"]),
    ("package-cpp", &["package-cpp", "node-cpp"]),
    ("package-python", &["package-python", "node-python"]),
    ("package-csharp", &["package-csharp"]),
    ("package-rust", &["package-rust", "node-rust"]),
    ("group", &["group"]),
    ("workspace-cpp", &["workspace-cpp"]),
    ("workspace-python", &["workspace-python"]),
    ("workspace-csharp", &["workspace-csharp"]),
    ("workspace-rust", &["workspace-rust", "package-rust"]),
];

/// The files a preset makes in dest. Refused when one is there already.
pub fn plan(preset: &str, dest: &Path, values: &Values) -> Result<Files> {
    let Some((_, folders)) = PRESETS.iter().find(|(name, _)| *name == preset) else {
        return Err(failed(format!("no preset `{preset}`")));
    };
    let mut files = Files::new();
    for folder in *folders {
        let prefix = format!("{folder}/");
        for (path, data) in FILES {
            if let Some(rel) = path.strip_prefix(&prefix) {
                files.push((paths::normalize(&dest.join(filled(rel, values))), filled(&String::from_utf8_lossy(data), values)));
            }
        }
    }
    if let Some((p, _)) = files.iter().find(|(p, _)| p.exists()) {
        return Err(failed(format!("{} exists already, nothing was written", paths::slash(p))));
    }
    Ok(files)
}

pub fn write(files: &Files) -> Result<()> {
    for (path, text) in files {
        let made = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(path, text));
        made.map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(path))))?;
    }
    Ok(())
}

fn filled(text: &str, values: &Values) -> String {
    values.iter().fold(text.to_string(), |t, (key, value)| t.replace(&format!("{{{{{key}}}}}"), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rant-scaffold-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_preset_writes_its_folders_with_the_name_and_never_overwrites() {
        let t = scratch("cpp");
        let values = Values::from([("name", "cam".to_string())]);
        let files = plan("package-cpp", &t.join("cam"), &values).unwrap();
        let names: Vec<String> = files.iter().map(|(p, _)| paths::relative(p, &t)).collect();
        assert_eq!(names, ["cam/CMakeLists.txt", "cam/cam.cpp"]);
        write(&files).unwrap();
        let cmake = std::fs::read_to_string(t.join("cam/CMakeLists.txt")).unwrap();
        assert!(cmake.contains("project(cam CXX)\n"), "{cmake}");
        assert!(cmake.contains("add_executable(cam cam.cpp)\ntarget_link_libraries(cam PRIVATE rant::rant_host)\n"), "{cmake}");
        assert!(std::fs::read_to_string(t.join("cam/cam.cpp")).unwrap().contains("rant::Node node(\"cam\");"));
        let again = plan("package-cpp", &t.join("cam"), &values).unwrap_err();
        assert!(again.messages()[0].ends_with("cam/CMakeLists.txt exists already, nothing was written"));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_csharp_project_is_pascal_case_and_targets_the_framework_given() {
        let t = scratch("csharp");
        let values = Values::from([("name", "panel".to_string()), ("Name", "Panel".to_string()), ("framework", "net10.0".to_string())]);
        write(&plan("package-csharp", &t.join("panel"), &values).unwrap()).unwrap();
        let project = std::fs::read_to_string(t.join("panel/Panel.csproj")).unwrap();
        assert!(project.contains("<TargetFramework>net10.0</TargetFramework>"));
        assert!(project.contains("<AssemblyName>panel</AssemblyName>"));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_workspace_writes_its_files_in_path_order_and_no_editor_data() {
        let values = Values::from([("name", "rusty".to_string()), ("Name", "Rusty".to_string())]);
        let t = scratch("rust");
        let files = plan("workspace-rust", &t, &values).unwrap();
        let names: Vec<String> = files.iter().map(|(p, _)| paths::relative(p, &t)).collect();
        assert_eq!(names, ["src/bin/listener.rs", "src/bin/talker.rs", "workspace.rant", "Rusty.run", ".gitignore", "Cargo.toml"]);
    }

    #[test]
    fn a_preset_that_does_not_exist_is_an_error() {
        let err = plan("nothing", Path::new("."), &Values::new()).unwrap_err();
        assert_eq!(err.messages(), ["no preset `nothing`"]);
    }
}
