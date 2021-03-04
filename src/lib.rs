extern crate itertools;
use itertools::Itertools;

#[macro_use]
extern crate failure_derive;
extern crate failure;
use failure::Error;

extern crate serde;

#[macro_use]
extern crate lazy_static;

extern crate toml;

use swayipc::{Connection, Node, NodeType, WindowChange, WindowEvent, WorkspaceChange, WorkspaceEvent};

use std::collections::HashMap as Map;

pub mod config;
pub mod icons;

pub struct Config {
    pub icons: Map<String, char>,
    pub aliases: Map<String, String>,
    pub general: Map<String, String>,
    pub options: Map<String, bool>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            icons: icons::NONE.clone(),
            aliases: config::EMPTY_MAP.clone(),
            general: config::EMPTY_MAP.clone(),
            options: config::EMPTY_OPT_MAP.clone(),
        }
    }
}

#[derive(Debug, Fail)]
enum LookupError {
    #[fail(
        display = "Failed to get app_id or window_properties for node: {:#?}",
        _0
    )]
    MissingInformation(String),
    #[fail(display = "Failed to get name for workspace: {:#?}", _0)]
    WorkspaceName(Box<Node>),
}

fn get_option(config: &Config, key: &str) -> bool {
    return match config.options.get(key) {
        Some(v) => *v,
        None => false,
    };
}

fn get_class(node: &Node, config: &Config) -> Result<String, LookupError> {
    let name = {
        match &node.app_id {
            Some(id) => Some(id.to_owned()),
            None => match &node.window_properties {
                Some(properties) => Some(properties.class.as_ref().unwrap().to_owned()),
                None => None,
            },
        }
    };
    if let Some(class) = name {
        let class_display_name = match config.aliases.get(&class) {
            Some(alias) => alias,
            None => &class,
        };

        let no_names = get_option(&config, "no_names");

        Ok(match config.icons.get(&class) {
            Some(icon) => {
                if no_names {
                    format!("{}", icon)
                } else {
                    format!("{} {}", icon, class_display_name)
                }
            }
            None => match config.general.get("default_icon") {
                Some(default_icon) => {
                    if no_names {
                        format!("{}", default_icon)
                    } else {
                        format!("{} {}", default_icon, class_display_name)
                    }
                }
                None => {
                    format!("{}", class_display_name)
                }
            },
        })
    } else {
        Err(LookupError::MissingInformation(format!("{:?}", node)))
    }
}

/// return a collection of workspace nodes
fn get_workspaces(tree: Node) -> Vec<Node> {
    let mut out = Vec::new();

    for output in tree.nodes {
        for container in output.nodes {
            if let NodeType::Workspace = container.node_type {
                out.push(container);
            }
        }
    }

    out
}

/// get all nodes for any depth collection of nodes
fn get_window_nodes(mut nodes: Vec<Vec<&Node>>) -> Vec<&Node> {
    let mut window_nodes = Vec::new();

    while let Some(next) = nodes.pop() {
        for n in next {
            nodes.push(n.nodes.iter().collect());
            if let Some(_) = n.window {
                window_nodes.push(n);
            } else if let Some(_) = n.app_id {
                window_nodes.push(n);
            }
        }
    }

    window_nodes
}

/// Return a collection of window classes
fn get_classes(workspace: &Node, config: &Config) -> Vec<String> {
    let window_nodes = {
        let mut f = get_window_nodes(vec![workspace.floating_nodes.iter().collect()]);
        let mut n = get_window_nodes(vec![workspace.nodes.iter().collect()]);
        n.append(&mut f);
        n
    };

    let mut window_classes = Vec::new();
    let focused_only = get_option(&config, "focused_only");

    for node in window_nodes {
        let is_focused = node.focused;
        // Requires PR #27 from swayipc-rs to get merged
        // Dependancy should use crate when merged
        let is_visible = node.visible.unwrap();
        if focused_only && is_visible && !is_focused {
            continue;
        }
        let class = match get_class(node, config) {
            Ok(class) => class,
            Err(e) => {
                eprintln!("get class error: {}", e);
                continue;
            }
        };
        window_classes.push(class);
    }

    window_classes
}

/// Update all workspace names in tree
pub fn update_tree(connection: &mut Connection, config: &Config) -> Result<(), Error> {
    let tree = connection.get_tree()?;
    for workspace in get_workspaces(tree) {
        let separator = match config.general.get("separator") {
            Some(s) => s,
            None => " | ",
        };

        let classes = get_classes(&workspace, config);
        let classes = if get_option(&config, "remove_duplicates") {
            classes.into_iter().unique().collect()
        } else {
            classes
        };

        let classes = classes.join(separator);
        let classes = if !classes.is_empty() {
            format!(" {}", classes)
        } else {
            classes
        };

        let old: String = workspace
            .name
            .to_owned()
            .ok_or_else(|| LookupError::WorkspaceName(Box::new(workspace)))?;

        let mut new = old.split(' ').next().unwrap().to_owned();

        if !classes.is_empty() {
            new.push_str(&classes);
        }

        if old != new {
            let command = format!("rename workspace \"{}\" to \"{}\"", old, new);
            connection.run_command(&command)?;
        }
    }
    Ok(())
}

pub fn handle_window_event(
    event: &WindowEvent,
    connection: &mut Connection,
    config: &Config,
) -> Result<(), Error> {
    match event.change {
        WindowChange::New | WindowChange::Close | WindowChange::Move | WindowChange::Focus => {
            update_tree(connection, config)
        }
        _ => Ok(()),
    }
}

pub fn handle_workspace_event(
    event: &WorkspaceEvent,
    connection: &mut Connection,
    config: &Config,
) -> Result<(), Error> {
    match event.change {
        WorkspaceChange::Empty | WorkspaceChange::Focus => update_tree(connection, config),
        _ => Ok(()),
    }
}
