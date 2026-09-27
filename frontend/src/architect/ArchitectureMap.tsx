import React, { useEffect, useCallback } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type NodeProps,
  Handle,
  Position,
  MarkerType,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import dagre from "dagre";
import { useArchitect, type ArchitectLayer, type FileNodeInfo } from "./architectStore";

// ─── 1. Custom Layer Node Component (Phase 1) ────────────────────
const LayerNodeComponent = React.memo(({ data, selected }: NodeProps) => {
  const layer = data.layer as ArchitectLayer;
  const isSelected = selected;

  const getLayerColor = (type: string) => {
    switch (type) {
      case "frontend":
        return { accent: "#38bdf8", bg: "rgba(56, 189, 248, 0.12)", border: "rgba(56, 189, 248, 0.35)" };
      case "backend":
        return { accent: "#a855f7", bg: "rgba(168, 85, 247, 0.12)", border: "rgba(168, 85, 247, 0.35)" };
      case "database":
        return { accent: "#34d399", bg: "rgba(52, 211, 153, 0.12)", border: "rgba(52, 211, 153, 0.35)" };
      case "infra":
        return { accent: "#fb923c", bg: "rgba(251, 146, 60, 0.12)", border: "rgba(251, 146, 60, 0.35)" };
      case "shared":
      default:
        return { accent: "#fa586a", bg: "rgba(250, 88, 106, 0.12)", border: "rgba(250, 88, 106, 0.35)" };
    }
  };

  const colors = getLayerColor(layer.layer_type);

  return (
    <div
      className={`architect-layer-node ${isSelected ? "architect-layer-node--selected" : ""}`}
      style={{
        background: colors.bg,
        borderColor: isSelected ? "#fa586a" : colors.border,
        boxShadow: isSelected ? "0 0 24px rgba(250, 88, 106, 0.45)" : "0 8px 32px rgba(0, 0, 0, 0.5)",
      }}
    >
      <Handle type="target" position={Position.Top} className="architect-handle" />

      <div className="architect-node-header">
        <div className="architect-node-icon" style={{ backgroundColor: colors.accent }}>
          {layer.layer_type === "frontend" && "💻"}
          {layer.layer_type === "backend" && "⚡"}
          {layer.layer_type === "database" && "🗄️"}
          {layer.layer_type === "infra" && "☁️"}
          {layer.layer_type === "shared" && "📦"}
        </div>
        <div className="architect-node-title-box">
          <div className="architect-node-label">{layer.label}</div>
          <div className="architect-node-stack">{layer.tech_stack}</div>
        </div>
        <span className="architect-node-badge" style={{ color: colors.accent }}>
          {layer.file_count} files
        </span>
      </div>

      <div className="architect-node-body">
        <div className="architect-node-section-label">Key Directories:</div>
        <div className="architect-node-dirs">
          {layer.dirs.map((d, i) => (
            <span key={i} className="architect-dir-tag">
              {d}
            </span>
          ))}
        </div>
        {layer.sample_files.length > 0 && (
          <div className="architect-sample-files">
            {layer.sample_files.slice(0, 3).map((f, i) => (
              <div key={i} className="architect-file-item">
                📄 {f}
              </div>
            ))}
          </div>
        )}
      </div>

      <Handle type="source" position={Position.Bottom} className="architect-handle" />
    </div>
  );
});

// ─── 2. Custom File Node Component (Phase 2) ─────────────────────
const FileNodeComponent = React.memo(({ data, selected }: NodeProps) => {
  const file = data.file as FileNodeInfo;
  const isSelected = selected;

  const fileName = file.file_path.split("/").pop() || file.file_path;
  const dirName = file.file_path.includes("/")
    ? file.file_path.substring(0, file.file_path.lastIndexOf("/"))
    : "";

  let badgeColor = "#8e8e93";
  let borderColor = "rgba(255, 255, 255, 0.12)";
  let bg = "rgba(28, 28, 30, 0.85)";

  if (file.risk_level === "critical") {
    badgeColor = "#ff453a";
    borderColor = "rgba(255, 69, 58, 0.4)";
    bg = "rgba(255, 69, 58, 0.15)";
  } else if (file.risk_level === "high") {
    badgeColor = "#ff9f0a";
    borderColor = "rgba(255, 159, 10, 0.4)";
    bg = "rgba(255, 159, 10, 0.15)";
  } else if (file.risk_level === "medium") {
    badgeColor = "#0a84ff";
    borderColor = "rgba(10, 132, 255, 0.3)";
    bg = "rgba(10, 132, 255, 0.1)";
  }

  if (file.is_circular) {
    borderColor = "#bf5af2";
  }

  return (
    <div
      className={`architect-file-node ${isSelected ? "architect-file-node--selected" : ""} ${file.is_hotspot ? "architect-file-node--hotspot" : ""} ${file.is_circular ? "architect-file-node--circular" : ""}`}
      style={{
        background: bg,
        borderColor: isSelected ? "#fa586a" : borderColor,
        boxShadow: isSelected
          ? "0 0 24px rgba(250, 88, 106, 0.5)"
          : file.is_hotspot
          ? "0 0 16px rgba(255, 69, 58, 0.3)"
          : "0 4px 16px rgba(0, 0, 0, 0.4)",
      }}
    >
      <Handle type="target" position={Position.Top} className="architect-handle" />

      <div className="architect-file-header">
        <span className="architect-file-icon">📄</span>
        <div className="architect-file-name-box">
          <div className="architect-file-name">{fileName}</div>
          {dirName && <div className="architect-file-dir">{dirName}/</div>}
        </div>
        {file.in_degree > 0 && (
          <span className="architect-file-indegree" style={{ color: badgeColor }}>
            📥 {file.in_degree}
          </span>
        )}
      </div>

      <div className="architect-file-meta">
        {file.is_hotspot && (
          <span className="architect-risk-tag" style={{ color: badgeColor, borderColor: badgeColor }}>
            🔥 HOTSPOT ({file.risk_level.toUpperCase()})
          </span>
        )}
        {file.is_circular && <span className="architect-cycle-tag">🔄 CYCLE</span>}
        <span className="architect-dep-summary">
          {file.imports.length} imports • {file.imported_by.length} used by
        </span>
      </div>

      <Handle type="source" position={Position.Bottom} className="architect-handle" />
    </div>
  );
});

const nodeTypes = {
  layerNode: LayerNodeComponent,
  fileNode: FileNodeComponent,
};

// ─── Dagre Auto Layout Helper ────────────────────────────────────
function getLayoutedElements(nodes: Node[], edges: Edge[], isFileMode = false) {
  const dagreGraph = new dagre.graphlib.Graph();
  dagreGraph.setDefaultEdgeLabel(() => ({}));
  dagreGraph.setGraph({
    rankdir: "TB",
    nodesep: isFileMode ? 40 : 60,
    ranksep: isFileMode ? 60 : 90,
  });

  const nodeWidth = isFileMode ? 240 : 320;
  const nodeHeight = isFileMode ? 100 : 220;

  nodes.forEach((node) => {
    dagreGraph.setNode(node.id, { width: nodeWidth, height: nodeHeight });
  });

  edges.forEach((edge) => {
    dagreGraph.setEdge(edge.source, edge.target);
  });

  dagre.layout(dagreGraph);

  const layoutedNodes = nodes.map((node) => {
    const nodeWithPosition = dagreGraph.node(node.id);
    return {
      ...node,
      position: {
        x: nodeWithPosition.x - nodeWidth / 2,
        y: nodeWithPosition.y - nodeHeight / 2,
      },
    };
  });

  return { nodes: layoutedNodes, edges };
}

// ─── Main Map Component ──────────────────────────────────────────
export function ArchitectureMap() {
  const phase1Data = useArchitect((s) => s.phase1Data);
  const phase2Data = useArchitect((s) => s.phase2Data);
  const viewMode = useArchitect((s) => s.viewMode);
  const selectedNodeId = useArchitect((s) => s.selectedNodeId);
  const highlightedPaths = useArchitect((s) => s.highlightedPaths);
  const setSelectedNodeId = useArchitect((s) => s.setSelectedNodeId);

  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  // Construct highlighted edge set
  const highlightedEdgeSet = React.useMemo(() => {
    const set = new Set<string>();
    for (const path of highlightedPaths) {
      for (let i = 0; i < path.length - 1; i++) {
        set.add(`${path[i]}->${path[i + 1]}`);
        set.add(`${path[i + 1]}->${path[i]}`);
      }
    }
    return set;
  }, [highlightedPaths]);

  // Construct ReactFlow nodes and edges based on phase and viewMode.
  // NOTE: `selectedNodeId` is intentionally excluded from this effect's
  // deps so clicking a node doesn't re-run the expensive dagre layout.
  // Selection is applied via the separate effect below.
  useEffect(() => {
    // 1. Phase 2 Detailed File Dependency Graph View
    if (phase2Data && (viewMode === "files" || viewMode === "hotspots" || viewMode === "cycles")) {
      let filteredEntries = Object.entries(phase2Data.nodes);

      if (viewMode === "hotspots") {
        filteredEntries = filteredEntries.filter(([_, f]) => f.is_hotspot);
      } else if (viewMode === "cycles") {
        filteredEntries = filteredEntries.filter(([_, f]) => f.is_circular);
      } else {
        // In full file mode, prioritize top 75 files to maintain smooth performance
        filteredEntries = filteredEntries
          .sort((a, b) => b[1].in_degree - a[1].in_degree)
          .slice(0, 75);
      }

      const activeFileSet = new Set(filteredEntries.map(([path]) => path));

      const rawNodes: Node[] = filteredEntries.map(([path, file]) => ({
        id: path,
        type: "fileNode",
        data: { file },
        position: { x: 0, y: 0 },
      }));

      const rawEdges: Edge[] = [];
      filteredEntries.forEach(([path, file]) => {
        file.imports.forEach((targetPath) => {
          if (activeFileSet.has(targetPath)) {
            const edgeKey = `${path}->${targetPath}`;
            const isHighlighted = highlightedEdgeSet.has(edgeKey);

            rawEdges.push({
              id: `e_${path}_${targetPath}`,
              source: path,
              target: targetPath,
              type: "smoothstep",
              animated: isHighlighted || file.is_circular,
              style: {
                stroke: isHighlighted ? "#fa586a" : file.is_circular ? "#bf5af2" : "rgba(255, 255, 255, 0.2)",
                strokeWidth: isHighlighted ? 3 : 1.5,
              },
              markerEnd: {
                type: MarkerType.ArrowClosed,
                color: isHighlighted ? "#fa586a" : file.is_circular ? "#bf5af2" : "rgba(255, 255, 255, 0.4)",
              },
            });
          }
        });
      });

      const layouted = getLayoutedElements(rawNodes, rawEdges, true);
      setNodes(layouted.nodes);
      setEdges(layouted.edges);
      return;
    }

    // 2. Phase 1 Layer View (Default)
    if (phase1Data && phase1Data.layers) {
      const rawNodes: Node[] = phase1Data.layers.map((layer) => ({
        id: layer.id,
        type: "layerNode",
        data: { layer },
        position: { x: 0, y: 0 },
      }));

      const rawEdges: Edge[] = phase1Data.edges.map((edge) => ({
        id: edge.id,
        source: edge.source,
        target: edge.target,
        label: edge.label,
        type: "smoothstep",
        animated: true,
        style: { stroke: "rgba(250, 88, 106, 0.65)", strokeWidth: 2 },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: "#fa586a",
        },
        labelStyle: { fill: "#ffffff", fontSize: 11, fontWeight: 600 },
        labelBgStyle: { fill: "rgba(18, 18, 20, 0.85)", stroke: "rgba(255, 255, 255, 0.15)" },
      }));

      const layouted = getLayoutedElements(rawNodes, rawEdges, false);
      setNodes(layouted.nodes);
      setEdges(layouted.edges);
    }
  }, [phase1Data, phase2Data, viewMode, highlightedEdgeSet, setNodes, setEdges]);

  // Apply selection state without re-running dagre layout.
  useEffect(() => {
    setNodes((nds) =>
      nds.map((n) => ({ ...n, selected: n.id === selectedNodeId }))
    );
  }, [selectedNodeId, setNodes]);

  const onNodeClick = useCallback(
    (_: React.MouseEvent, node: Node) => {
      setSelectedNodeId(node.id);
    },
    [setSelectedNodeId]
  );

  const onPaneClick = useCallback(() => {
    setSelectedNodeId(null);
  }, [setSelectedNodeId]);

  return (
    <div className="architect-map-canvas">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeClick={onNodeClick}
        onPaneClick={onPaneClick}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        minZoom={0.15}
        maxZoom={2.2}
        colorMode="dark"
        style={{ background: "transparent" }}
      >
        <Background color="rgba(255, 255, 255, 0.05)" gap={20} size={1} />
        <Controls className="architect-flow-controls" />
        <MiniMap
          className="architect-flow-minimap"
          nodeColor={() => "#fa586a"}
          maskColor="rgba(0, 0, 0, 0.75)"
        />
      </ReactFlow>
    </div>
  );
}
