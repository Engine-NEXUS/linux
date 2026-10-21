# Centralized API Key Management System Design

## Overview
A centralized API key management system that allows users to store all API keys in one place (except Google and model API keys like Groq/Gemini which are handled separately). This provides a single source of truth for all API credentials used by the NEXUS command center.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CENTRALIZED API KEY MANAGEMENT                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────┐    ┌─────────────────┐    ┌────────────────────────┐  │
│  │   User Config   │───▶│  Key Vault      │───▶│  Service Consumers     │  │
│  │   (Settings)    │    │  (Encrypted)    │    │  • STT (Groq)          │  │
│  │                 │    │                 │    │  • TTS (Edge/Piper)    │  │
│  │  • Groq API     │    │  • Groq API     │    │  • LLM (Groq/Gemini)   │  │
│  │  • GitHub Token │    │  • GitHub Token │    │  • Search (Tavily)     │  │
│  │  • Tavily Key   │    │  • Tavily Key   │    │  • Weather API         │  │
│  │  • Weather Key  │    │  • Weather Key  │    │  • News API            │  │
│  │  • ...          │    │  • ...          │    │  • ...                 │  │
│  └─────────────────┘    └─────────────────┘    └────────────────────────┘  │
│                                                                             │
│  EXCLUDED (User manages separately):                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • Google OAuth (handled by existing OAuth flow)                     │   │
│  │  • Model API Keys: Groq, Gemini (managed by existing config)        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Model

### API Key Entry
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub id: String,                    // UUID
    pub key_name: String,              // e.g., "groq", "github", "tavily"
    pub display_name: String,          // Human-readable: "Groq API", "GitHub Token"
    pub category: ApiKeyCategory,      // STT, TTS, LLM, SEARCH, GITHUB, WEATHER, etc.
    pub encrypted_value: String,       // AES-256-GCM encrypted
    pub key_hint: String,              // Last 4 chars: "sk-...abcd"
    pub created_at: u64,               // Unix timestamp
    pub updated_at: u64,               // Unix timestamp
    pub last_used_at: Option<u64>,     // Last usage timestamp
    pub is_active: bool,               // Enable/disable without deletion
    pub metadata: HashMap<String, String>, // Additional metadata
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ApiKeyCategory {
    Stt,           // Speech-to-Text (Groq Whisper)
    Tts,           // Text-to-Speech (Edge, Piper)
    Llm,           // LLM Providers (Groq, Gemini, OpenAI)
    Search,        // Search APIs (Tavily, Serper, Brave)
    Github,        // GitHub Personal Access Token
    Weather,       // Weather API (OpenWeather, WeatherAPI)
    News,          // News API (NewsAPI, GNews)
    Maps,          // Maps/Geocoding (Google Maps, Mapbox)
    Email,         // Email (SendGrid, Mailgun)
    Custom(String), // Extensible
}
```

### Key Vault Structure
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyVault {
    pub version: u32,                    // Schema version
    pub keys: HashMap<String, ApiKeyEntry>, // key_name -> entry
    pub key_order: Vec<String>,          // Display order
    pub master_key_hash: String,         // Argon2id hash of master password
    pub salt: String,                    // Argon2 salt
    pub created_at: u64,
    pub updated_at: u64,
}
```

## Security Design

### Encryption
- **Algorithm**: AES-256-GCM (authenticated encryption)
- **Key Derivation**: Argon2id (memory-hard, resistant to GPU cracking)
  - Memory: 64 MB
  - Iterations: 3
  - Parallelism: 4
  - Output: 32 bytes (256-bit key)
- **Nonce**: 96-bit random per encryption
- **Associated Data**: Key name + category (prevents key substitution)

### Key Derivation
```
master_key = Argon2id(
    password: user_master_password,
    salt: vault_salt,
    memory: 64 MiB,
    iterations: 3,
    parallelism: 4,
    output_len: 32
)

encryption_key = HKDF-SHA256(
    ikm: master_key,
    salt: "nexus-api-key-v1",
    info: key_name + category,
    length: 32
)
```

### Storage
- **File**: `%APPDATA%/com.nexus.assistant/keyvault.json`
- **Backup**: Encrypted export/import functionality
- **Migration**: Versioned schema with migration support

## User Experience

### Settings UI Flow
```
Settings → API Keys
├── Groq API Key          [●●●●●●●●●●●●abcd]  [Test] [Edit] [Delete]
├── GitHub Token          [●●●●●●●●●●●●abcd]  [Test] [Edit] [Delete]
├── Tavily API Key        [●●●●●●●●●●●●abcd]  [Test] [Edit] [Delete]
├── Weather API Key       [●●●●●●●●●●●●abcd]  [Test] [Edit] [Delete]
├── [+ Add New Key] ▼
│   ├── Search API
│   ├── Weather API
│   ├── News API
│   ├── Email API
│   └── Custom...
└── [Import Keys] [Export Keys] [Reset All]
```

### Key Features
1. **Masked Display**: Show only last 4 characters
2. **Test Button**: Validate key works before saving
3. **Category Filtering**: Filter by STT/TTS/LLM/Search/etc.
3. **Usage Tracking**: Last used timestamp, usage count
4. **Import/Export**: Encrypted backup/restore
5. **Key Rotation**: Easy rotation with history

## Rust Implementation

### Core Vault Module
```rust
// src/api_keys/vault.rs
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use argon2::{Argon2, Params, Algorithm, Version};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

pub struct KeyVault {
    keys: HashMap<String, ApiKeyEntry>,
    key_order: Vec<String>,
    master_key_hash: String,
    salt: String,
    path: PathBuf,
}

impl KeyVault {
    pub fn new(path: PathBuf) -> Result<Self, Error> { ... }
    
    pub fn unlock(&mut self, master_password: &str) -> Result<(), Error> { ... }
    
    pub fn add_key(&mut self, entry: ApiKeyEntry) -> Result<(), Error> { ... }
    
    pub fn get_key(&self, key_name: &str) -> Option<&str> { ... }
    
    pub fn update_key(&mut self, key_name: &str, new_value: &str) -> Result<(), Error> { ... }
    
    pub fn delete_key(&mut self, key_name: &str) -> Result<(), Error> { ... }
    
    pub fn list_keys(&self) -> Vec<ApiKeySummary> { ... }
    
    pub fn test_key(&self, key_name: &str) -> Result<KeyTestResult, Error> { ... }
    
    pub fn export_encrypted(&self, export_password: &str) -> Result<Vec<u8>, Error> { ... }
    
    pub fn import_encrypted(&mut self, data: &[u8], import_password: &str) -> Result<(), Error> { ... }
}
```

### Integration with Orchestrator
```rust
// In orchestrator.rs or a new api_keys module
pub async fn get_api_key(category: ApiKeyCategory, key_name: &str) -> Result<String, String> {
    let vault = KEY_VAULT.lock().await;
    vault.get_decrypted(key_name)
}

pub async fn set_api_key(category: ApiKeyCategory, key_name: &str, value: &str) -> Result<(), String> {
    let mut vault = KEY_VAULT.lock().await;
    vault.add_or_update(ApiKeyEntry { ... })
}
```

## Tauri Commands

```rust
#[tauri::command]
async fn get_api_keys() -> Result<Vec<ApiKeySummary>, String> { ... }

#[tauri::command]
async fn add_api_key(entry: ApiKeyEntry) -> Result<(), String> { ... }

#[tauri::command]
async fn update_api_key(name: &str, value: &str) -> Result<(), String> { ... }

#[tauri::command]
async fn delete_api_key(name: &str) -> Result<(), String> { ... }

#[tauri::command]
async fn test_api_key(category: ApiKeyCategory, key: &str) -> Result<bool, String> { ... }

#[tauri::command]
async fn export_keys(password: &str) -> Result<Vec<u8>, String> { ... }

#[tauri::command]
async fn import_keys(data: Vec<u8>, password: &str) -> Result<(), String> { ... }
```

## Frontend Integration

### Settings Page Component
```tsx
// SettingsSidebarApp.tsx - API Keys Section
const APIKeyManager = () => {
  const [keys, setKeys] = useState<ApiKeySummary[]>([]);
  const [showModal, setShowModal] = useState(false);
  const [editingKey, setEditingKey] = useState<ApiKeySummary | null>(null);
  
  // Load keys on mount
  useEffect(() => {
    invoke('get_api_keys').then(setKeys).catch(console.error);
  }, []);
  
  const handleAddKey = async (entry: ApiKeyEntry) => {
    await invoke('add_api_key', { entry });
    // Refresh list
  };
  
  const handleTestKey = async (category: ApiKeyCategory, key: string) => {
    const result = await invoke('test_api_key', { category, key });
    // Show success/error toast
  };
  
  return (
    <section className="settings-section">
      <h2>API Keys</h2>
      <p className="hint">Keys are encrypted with your master password. Google & Model APIs managed separately.</      <KeyList keys={keys} onEdit={setEditingKey} onDelete={handleDelete} onTest={handleTestKey} />
      <button onClick={() => setShowModal(true)}>+ Add API Key</button>
      {showModal && <KeyModal onClose={() => setShowModal(false)} onSave={handleAddKey} />}
    </section>
  );
};
```

## Migration Strategy

### Phase 1: Core Vault (Week 1)
- [ ] KeyVault struct with encryption/decryption
- [ ] Tauri commands for CRUD operations
- [ ] Basic Settings UI for API keys

### Phase 2: Integration (Week 2)
- [ ] Wire into STT/TTS/Orchestrator
- [ ] Add key testing functionality
- [ ] Import/Export functionality

### Phase 3: Advanced Features (Week 3)
- [ ] Usage analytics dashboard
- [ ] Key rotation workflow
- [ ] Encrypted backup/restore
- [ ] Team sharing (future)

## Security Considerations

1. **Never log API keys** - Use key_hint (last 4 chars) only
2. **Master password never stored** - Only Argon2id hash
3. **Per-key encryption** - Different nonce per key
4. **Audit logging** - All access logged (without values)
3. **Auto-lock** - Vault locks after 15 min inactivity
4. **Secure memory** - Zeroize keys on lock

## Migration from Current System

```rust
// Migration from scattered config to centralized vault
async fn migrate_to_vault() -> Result<(), Error> {
    let mut vault = KeyVault::new(vault_path)?;
    
    // Migrate Groq API key
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        vault.add_key(ApiKeyEntry {
            key_name: "groq".into(),
            display_name: "Groq API".into(),
            category: ApiKeyCategory::Stt,
            encrypted_value: encrypt(&key)?,
            key_hint: mask_key(&key),
            // ...
        })?;
    }
    
    // ... migrate other keys
    
    Ok(())
}
```

## Summary

| Feature | Status |
|---------|--------|
| Encrypted storage (AES-256-GCM + Argon2id) | ✅ Planned |
| Tauri commands for CRUD | ✅ Planned |
| Settings UI integration | ✅ Planned |
| Key testing/validation | ✅ Planned |
| Import/Export (encrypted) | ✅ Planned |
| Orchestrator integration | ✅ Planned |
| Auto-migration from env vars | ✅ Planned |
| Master password + auto-lock | ✅ Planned |

This design centralizes all API keys (except Google/Model keys) in one encrypted, user-friendly vault with full CRUD, testing, and audit capabilities.