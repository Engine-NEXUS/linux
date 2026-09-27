import { create } from "zustand";

export interface ArchitectLayer {
  id: string;
  label: string;
  layer_type: "frontend" | "backend" | "database" | "infra" | "shared" | string;
  dirs: string[];
  tech_stack: string;
  file_count: number;
  sample_files: string[];
}

export interface ArchitectEdge {
  id: string;
  source: string;
  target: string;
  label?: string;
  edge_type?: "imports" | "calls" | "configures" | string;
}

export interface Phase1Data {
  owner: string;
  repo: string;
  default_branch: string;
  primary_language: string;
  description: string;
  summary: string;
  layers: ArchitectLayer[];
  edges: ArchitectEdge[];
  entry_points: string[];
  total_files: number;
  /** Sample file paths for LLM enrichment (not used for rendering). */
  sample_file_paths?: string[];
}

export interface FileNodeInfo {
  file_path: string;
  layer_id?: string | null;
  in_degree: number;
  out_degree: number;
  imports: string[];
  imported_by: string[];
  is_hotspot: boolean;
  risk_level: "normal" | "medium" | "high" | "critical" | string;
  is_circular: boolean;
}

export interface CircularDependency {
  chain: string[];
  risk: string;
}

export interface HotspotItem {
  file: string;
  in_degree: number;
  risk: "high" | "critical" | string;
}

export interface Phase2Data {
  owner: string;
  repo: string;
  total_files: number;
  files_analyzed: number;
  nodes: Record<string, FileNodeInfo>;
  circular_deps: CircularDependency[];
  hotspots: HotspotItem[];
  isolated: string[];
  entry_points: string[];
  summary: string;
}

/** Progressive analysis status messages received via Tauri Channel.
 *  Matches the Rust `ArchitectProgress` enum (serde tag = "type", PascalCase). */
export type ArchitectProgress =
  | { type: "Detecting"; owner: string; repo: string; message: string }
  | { type: "Indexing"; total_files: number; message: string }
  | { type: "GraphReady"; node_count: number; edge_count: number }
  | { type: "HotspotsReady"; hotspots: HotspotItem[] }
  | { type: "CyclesReady"; circular_deps: CircularDependency[] }
  | { type: "AiExplanation"; summary: string }
  | { type: "Complete"; stage: string }
  | { type: "Failed"; stage: string; error: string };

export type ViewMode = "layers" | "files" | "hotspots" | "cycles";

interface ArchitectState {
  owner: string;
  repo: string;
  phase: 0 | 1 | 2 | 3;
  loading: boolean;
  deepScanning: boolean;
  progressStage: string;
  progressMessage: string;
  phase1Data: Phase1Data | null;
  phase2Data: Phase2Data | null;
  selectedNodeId: string | null;
  highlightedPaths: string[][];
  viewMode: ViewMode;

  setRepo: (owner: string, repo: string) => void;
  setLoading: (loading: boolean) => void;
  setDeepScanning: (deepScanning: boolean) => void;
  setProgress: (stage: string, message: string) => void;
  setPhase1Data: (data: Phase1Data) => void;
  enrichPhase1: (enrichment: { summary: string; layers: { id: string; label: string; tech_stack: string }[] }) => void;
  setPhase2Data: (data: Phase2Data) => void;
  setSelectedNodeId: (nodeId: string | null) => void;
  setViewMode: (mode: ViewMode) => void;
  reset: () => void;
}

export const useArchitect = create<ArchitectState>((set) => ({
  owner: "",
  repo: "",
  phase: 0,
  loading: false,
  deepScanning: false,
  progressStage: "",
  progressMessage: "",
  phase1Data: null,
  phase2Data: null,
  selectedNodeId: null,
  highlightedPaths: [],
  viewMode: "layers",

  setRepo: (owner, repo) => set({ owner, repo }),
  setLoading: (loading) => set({ loading }),
  setDeepScanning: (deepScanning) => set({ deepScanning }),
  setProgress: (stage, message) => set({ progressStage: stage, progressMessage: message }),
  setPhase1Data: (data) =>
    set({
      phase1Data: data,
      owner: data.owner,
      repo: data.repo,
      phase: 1,
      loading: false,
      progressStage: "complete",
      progressMessage: "Phase 1 visual map ready",
    }),
  enrichPhase1: (enrichment) =>
    set((state) => {
      if (!state.phase1Data) return {};
      const updatedLayers = state.phase1Data.layers.map((layer) => {
        const enriched = enrichment.layers.find((e) => e.id === layer.id);
        if (!enriched) return layer;
        return {
          ...layer,
          label: enriched.label || layer.label,
          tech_stack: enriched.tech_stack || layer.tech_stack,
        };
      });
      return {
        phase1Data: {
          ...state.phase1Data,
          summary: enrichment.summary || state.phase1Data.summary,
          layers: updatedLayers,
        },
      };
    }),
  setPhase2Data: (data) =>
    set({
      phase2Data: data,
      phase: 2,
      deepScanning: false,
      progressStage: "complete",
      progressMessage: "Phase 2 deep scan complete",
    }),
  setSelectedNodeId: (selectedNodeId) => set({ selectedNodeId }),
  setViewMode: (viewMode) => set({ viewMode }),
  reset: () =>
    set({
      owner: "",
      repo: "",
      phase: 0,
      loading: false,
      deepScanning: false,
      progressStage: "",
      progressMessage: "",
      phase1Data: null,
      phase2Data: null,
      selectedNodeId: null,
      highlightedPaths: [],
      viewMode: "layers",
    }),
}));
