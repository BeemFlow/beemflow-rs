//! Graph generation and visualization
//!
//! Generates Mermaid diagrams from flow definitions for visual representation.

use crate::{Flow, Result, Step};

/// Node in the flow graph
#[derive(Debug, Clone)]
struct Node {
    id: String,
    label: String,
}

/// Edge connecting two nodes
#[derive(Debug, Clone)]
struct Edge {
    from: String,
    to: String,
    label: Option<String>,
}

/// Flow graph representation
#[derive(Debug, Clone)]
struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl Graph {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    fn add_node(&mut self, id: String, label: String) {
        self.nodes.push(Node { id, label });
    }

    fn add_edge(&mut self, from: String, to: String, label: Option<String>) {
        self.edges.push(Edge { from, to, label });
    }

    /// Process steps and build graph
    fn process_steps(&mut self, steps: &[Step], parent_id: Option<&str>) {
        for (i, step) in steps.iter().enumerate() {
            // Add node for this step
            let label = step
                .use_
                .as_ref()
                .map(|u| format!("{}\n{}", step.id, u))
                .unwrap_or_else(|| step.id.clone());
            self.add_node(step.id.clone(), label);

            // Handle parallel blocks
            if step.parallel == Some(true)
                && let Some(ref nested_steps) = step.steps
            {
                self.process_steps(nested_steps, Some(&step.id));
                continue;
            }

            // Handle foreach blocks
            if step.foreach.is_some()
                && let Some(ref do_steps) = step.do_
            {
                self.process_steps(do_steps, Some(&step.id));
                continue;
            }

            // Determine dependencies
            let deps: Vec<String> = if let Some(ref depends_on) = step.depends_on {
                depends_on.clone()
            } else if let Some(parent) = parent_id {
                vec![parent.to_string()]
            } else if i > 0 {
                vec![steps[i - 1].id.clone()]
            } else {
                Vec::new()
            };

            // Create edges for dependencies
            for dep in deps {
                self.add_edge(dep, step.id.clone(), None);
            }
        }
    }
}

/// Mermaid renderer for flow graphs
struct MermaidRenderer;

impl MermaidRenderer {
    fn render(&self, graph: &Graph) -> String {
        if graph.nodes.is_empty() {
            return String::new();
        }

        let mut output = String::from("graph TD\n");

        // Add Start and End nodes
        output.push_str("    Start([Start])\n");
        output.push_str("    End([End])\n");

        // Connect Start to first node
        if !graph.nodes.is_empty() {
            output.push_str(&format!("    Start --> {}\n", graph.nodes[0].id));
        }

        // Output node definitions
        for node in &graph.nodes {
            output.push_str(&format!("    {}[{}]\n", node.id, node.label));
        }

        // Output edges
        for edge in &graph.edges {
            if let Some(ref label) = edge.label {
                output.push_str(&format!("    {} -->|{}| {}\n", edge.from, label, edge.to));
            } else {
                output.push_str(&format!("    {} --> {}\n", edge.from, edge.to));
            }
        }

        // Connect last node to End
        if let Some(last_node) = graph.nodes.last() {
            output.push_str(&format!("    {} --> End\n", last_node.id));
        }

        output
    }
}

/// Graph generator for flow visualization
pub struct GraphGenerator;

impl GraphGenerator {
    /// Generate Mermaid diagram from flow
    pub fn generate(flow: &Flow) -> Result<String> {
        let mut graph = Graph::new();

        if !flow.steps.is_empty() {
            graph.process_steps(&flow.steps, None);
        }

        let renderer = MermaidRenderer;
        Ok(renderer.render(&graph))
    }
}
