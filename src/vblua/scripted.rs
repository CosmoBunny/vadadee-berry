//! Scripted node types (Phase 14): named template recipes.
//!
//! Lua shape:
//! ```lua
//! vblua.node.define("SoftGlow", {
//!   nodes = {
//!     { kind = "Blur", x = 0, y = 0 },
//!     { kind = "Brightness", x = 200, y = 0 },
//!   },
//!   links = {
//!     { from = 1, from_port = "out", to = 2, to_port = "in" },
//!   },
//! })
//! local ids = vblua.node.spawn("SoftGlow", { x = 100, y = 100 })
//! ```
//!
//! Posture (spec: engine owns lifecycle and GPU resources):
//! - Lua describes a recipe; the engine validates it at `define` time and
//!   owns the parsed template (kinds, ports, params — never Lua closures).
//! - `spawn` instantiates **ordinary host nodes** through the standard
//!   `GraphCommand` queue (one batch, one undo entry). Instances have no
//!   Lua-owned lifetime; per-frame `update` callbacks are future work that
//!   must go through engine-evaluated expressions, not script closures.
//! - Budgets: 64 templates, 32 nodes per template.

use std::collections::HashMap;

use crate::document::GraphNodeKind;

use super::graph::{ParamValue, parse_kind};

pub const MAX_TEMPLATES: usize = 64;
pub const MAX_TEMPLATE_NODES: usize = 32;

/// One node slot in a template (offsets relative to spawn origin).
#[derive(Debug, Clone)]
pub struct TemplateNode {
    pub kind: GraphNodeKind,
    pub kind_name: String,
    pub dx: f32,
    pub dy: f32,
    pub params: Vec<(String, ParamValue)>,
    pub name: Option<String>,
}

/// One internal wire (0-based slot indices).
#[derive(Debug, Clone)]
pub struct TemplateLink {
    pub from: usize,
    pub from_port: String,
    pub to: usize,
    pub to_port: String,
}

/// Engine-owned recipe (parsed + validated at define time).
#[derive(Debug, Clone)]
pub struct NodeTemplate {
    pub name: String,
    pub nodes: Vec<TemplateNode>,
    pub links: Vec<TemplateLink>,
}

/// Template registry (host state, persists across script runs).
#[derive(Debug, Clone, Default)]
pub struct TemplateRegistry {
    pub templates: HashMap<String, NodeTemplate>,
}

/// Raw Lua-decoded spec (validated by `compile_template` before storage).
pub struct RawTemplate {
    pub nodes: Vec<RawNode>,
    pub links: Vec<RawLink>,
}

pub struct RawNode {
    pub kind: String,
    pub dx: f32,
    pub dy: f32,
    pub name: Option<String>,
    pub params: Vec<(String, RawParam)>,
}

pub enum RawParam {
    Num(f64),
    Str(String),
}

pub struct RawLink {
    pub from: usize,
    pub from_port: String,
    pub to: usize,
    pub to_port: String,
}

/// Validate + compile a raw spec into an engine-owned template.
pub fn compile_template(name: &str, raw: RawTemplate) -> Result<NodeTemplate, String> {
    if name.is_empty() || name.len() > 64 {
        return Err("template name must be 1..64 chars".to_string());
    }
    if raw.nodes.is_empty() || raw.nodes.len() > MAX_TEMPLATE_NODES {
        return Err(format!("template needs 1..={MAX_TEMPLATE_NODES} nodes"));
    }
    let mut nodes = Vec::new();
    for (i, rn) in raw.nodes.into_iter().enumerate() {
        let kind = parse_kind(&rn.kind).map_err(|e| format!("node {}: {e}", i + 1))?;
        if !rn.dx.is_finite() || !rn.dy.is_finite() {
            return Err(format!("node {}: offsets must be finite", i + 1));
        }
        let mut params = Vec::new();
        if rn.params.len() > 64 {
            return Err(format!("node {}: at most 64 params", i + 1));
        }
        for (k, v) in rn.params {
            let pv = match v {
                RawParam::Num(n) => {
                    if !n.is_finite() {
                        return Err(format!("node {}: param '{k}' must be finite", i + 1));
                    }
                    ParamValue::Num(n)
                }
                RawParam::Str(s) => {
                    if s.is_empty() || s.len() > 512 {
                        return Err(format!("node {}: param '{k}' must be 1..512 chars", i + 1));
                    }
                    ParamValue::Str(s)
                }
            };
            // Validate key/type against the live kind now (fail at define).
            let mut probe = kind.clone();
            super::graph::set_node_param(&mut probe, &k, pv.clone())
                .map_err(|e| format!("node {}: {e}", i + 1))?;
            params.push((k, pv));
        }
        nodes.push(TemplateNode {
            kind: kind.clone(),
            kind_name: super::graph::kind_label(&kind).to_string(),
            dx: rn.dx.clamp(-4000.0, 4000.0),
            dy: rn.dy.clamp(-4000.0, 4000.0),
            params,
            name: rn.name.filter(|n| !n.is_empty() && n.len() <= 128),
        });
    }
    if raw.links.len() > 128 {
        return Err("template needs at most 128 links".to_string());
    }
    // Validate internal wires against parsed kinds (ports/dirs/types/cycles).
    let mut links = Vec::new();
    for (i, rl) in raw.links.into_iter().enumerate() {
        if rl.from >= nodes.len() || rl.to >= nodes.len() {
            return Err(format!("link {}: slot out of range", i + 1));
        }
        if rl.from == rl.to {
            return Err(format!("link {}: cannot wire a slot to itself", i + 1));
        }
        super::graph::validate_connect_kinds(
            &nodes.iter().map(|n| n.kind.clone()).collect::<Vec<_>>(),
            rl.from,
            &rl.from_port,
            rl.to,
            &rl.to_port,
        )
        .map_err(|e| format!("link {}: {e}", i + 1))?;
        links.push(TemplateLink {
            from: rl.from,
            from_port: rl.from_port,
            to: rl.to,
            to_port: rl.to_port,
        });
    }
    // Cycle check over slot indices.
    if template_has_cycle(nodes.len(), &links) {
        return Err("template wires form a cycle".to_string());
    }
    Ok(NodeTemplate {
        name: name.to_string(),
        nodes,
        links,
    })
}

fn template_has_cycle(n: usize, links: &[TemplateLink]) -> bool {
    let mut adj = vec![Vec::new(); n];
    for l in links {
        adj[l.from].push(l.to);
    }
    let mut state = vec![0u8; n];
    fn visit(adj: &[Vec<usize>], state: &mut [u8], v: usize) -> bool {
        if state[v] == 1 {
            return true;
        }
        if state[v] == 2 {
            return false;
        }
        state[v] = 1;
        for &w in &adj[v] {
            if visit(adj, state, w) {
                return true;
            }
        }
        state[v] = 2;
        false
    }
    (0..n).any(|v| visit(&adj, &mut state, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_two() -> RawTemplate {
        RawTemplate {
            nodes: vec![
                RawNode {
                    kind: "Blur".into(),
                    dx: 0.0,
                    dy: 0.0,
                    name: None,
                    params: vec![],
                },
                RawNode {
                    kind: "Brightness".into(),
                    dx: 200.0,
                    dy: 0.0,
                    name: Some("Lift".into()),
                    params: vec![],
                },
            ],
            links: vec![RawLink {
                from: 0,
                from_port: "out".into(),
                to: 1,
                to_port: "in".into(),
            }],
        }
    }

    #[test]
    fn compiles_and_rejects() {
        assert!(compile_template("Glow", raw_two()).is_ok());
        // Bad kind.
        let mut bad = raw_two();
        bad.nodes[0].kind = "Image".into();
        assert!(compile_template("Glow", bad).is_err());
        // Type mismatch (Value.out Real -> Blur.in RawImage).
        let mismatch = RawTemplate {
            nodes: vec![
                RawNode {
                    kind: "Value".into(),
                    dx: 0.0,
                    dy: 0.0,
                    name: None,
                    params: vec![],
                },
                RawNode {
                    kind: "Blur".into(),
                    dx: 0.0,
                    dy: 0.0,
                    name: None,
                    params: vec![],
                },
            ],
            links: vec![RawLink {
                from: 0,
                from_port: "out".into(),
                to: 1,
                to_port: "in".into(),
            }],
        };
        assert!(compile_template("Glow", mismatch).is_err());
        // Cycle.
        let mut cyc = raw_two();
        cyc.links.push(RawLink {
            from: 1,
            from_port: "out".into(),
            to: 0,
            to_port: "in".into(),
        });
        assert!(compile_template("Glow", cyc).is_err());
        // Empty + oversize.
        assert!(compile_template("", raw_two()).is_err());
        assert!(compile_template(
            "Glow",
            RawTemplate { nodes: vec![], links: vec![] }
        )
        .is_err());
    }
}
