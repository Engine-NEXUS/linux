/// <reference types="vite/client" />

interface ImportMetaEnv {
  // No VITE_ config surface — backend URL is hardcoded in Rust (WORKER_URL).
  // Identity comes from get_server_config at runtime.
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
