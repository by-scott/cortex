use crate::node_manager::remove_server_block;

#[test]
fn remove_server_block_drops_target_server() {
    let content = "\
[[servers]]
name = \"chrome-devtools\"
transport = \"stdio\"
command = \"npx\"

[[servers]]
name = \"other\"
transport = \"stdio\"
command = \"other\"
";

    let updated = remove_server_block(content, "chrome-devtools");

    assert!(!updated.contains("chrome-devtools"));
    assert!(updated.contains("name = \"other\""));
}
