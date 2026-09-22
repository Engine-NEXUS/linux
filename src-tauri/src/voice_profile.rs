// ─── Voice Profile: Speaker Verification ───────────────────────────
//
// Phase D: Owner-only wake word activation.
//
// Uses the existing openWakeWord embedding_model.onnx to extract speaker
// embeddings from detected audio. Compares to enrolled profile using
// cosine similarity. If similarity > threshold, the wake is accepted.
// If similarity < threshold, the wake is rejected silently.
//
// This is the same approach Apple Siri uses:
//   1. User says "NEXUS" 5 times during enrollment
//   2. Embeddings are extracted and stored as a voice profile
//   3. On wake detection, the detected audio's embedding is compared
//   4. If cosine similarity > threshold, accept; otherwise reject
//   5. Accepted utterances grow the profile (implicit enrollment, up to 40)
//
// The embedding model is the same one used for wake word detection
// (embedding_model.onnx), so no additional model files are needed.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Default cosine similarity threshold for speaker verification.
/// OVOS (Open Voice OS) uses 0.45 as default. Apple Siri uses a similar
/// threshold tuned for their proprietary embeddings.
///
/// Lower = more permissive (accepts more speakers)
/// Higher = more strict (only accepts the enrolled speaker)
pub const DEFAULT_THRESHOLD: f32 = 0.45;

/// Maximum number of embedding vectors to store in the profile.
/// Apple Siri grows the profile to 40 vectors via implicit enrollment.
/// We use the same limit.
pub const MAX_ENROLLMENT_VECTORS: usize = 40;

/// Minimum number of enrollment clips required.
pub const MIN_ENROLLMENT_CLIPS: usize = 3;

/// Sound-alike phrases that should NOT trigger wake word.
/// Used for negative training and display in the UI.
pub const SOUND_ALIKES: &[&str] = &[
    "next us", "texas", "lexus", "plex us", "alex us", "nexus 6p",
];

/// Voice profile stored on disk (JSON).
/// Contains enrolled speaker embeddings and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfile {
    /// Enrolled embedding vectors (96-dimensional each)
    pub embeddings: Vec<Vec<f32>>,
    /// Cosine similarity threshold for acceptance
    pub threshold: f32,
    /// Number of enrollment clips used
    pub num_clips: usize,
    /// Wake word variants captured during enrollment (e.g., "nexus", "hey nexus")
    pub wake_variants: Vec<String>,
    /// Profile creation timestamp (Unix epoch seconds)
    pub created_at: i64,
    /// Profile last updated timestamp (Unix epoch seconds)
    pub updated_at: i64,
}

impl VoiceProfile {
    /// Create a new empty voice profile.
    pub fn new(threshold: f32) -> Self {
        VoiceProfile {
            embeddings: Vec::new(),
            threshold,
            num_clips: 0,
            wake_variants: vec!["nexus".to_string()],
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Load a voice profile from a JSON file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read voice profile: {e}"))?;
        let profile: VoiceProfile = serde_json::from_str(&data)
            .map_err(|e| anyhow::anyhow!("Failed to parse voice profile: {e}"))?;
        Ok(profile)
    }

    /// Save the voice profile to a JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize voice profile: {e}"))?;
        std::fs::write(path, data)
            .map_err(|e| anyhow::anyhow!("Failed to write voice profile: {e}"))?;
        Ok(())
    }

    /// Add an embedding vector to the profile (implicit enrollment).
    /// If the profile is full (MAX_ENROLLMENT_VECTORS), the oldest vector
    /// is removed first (FIFO).
    pub fn add_embedding(&mut self, embedding: Vec<f32>) {
        if self.embeddings.len() >= MAX_ENROLLMENT_VECTORS {
            self.embeddings.remove(0); // Remove oldest (FIFO)
        }
        self.embeddings.push(embedding);
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Check if the profile is enrolled (has at least one embedding).
    pub fn is_enrolled(&self) -> bool {
        !self.embeddings.is_empty()
    }

    /// Get the number of enrolled embeddings.
    pub fn num_embeddings(&self) -> usize {
        self.embeddings.len()
    }
}

/// Status of the voice profile (returned to frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfileStatus {
    pub enrolled: bool,
    pub num_clips: usize,
    pub threshold: f32,
    pub created_at: i64,
    pub updated_at: i64,
    pub wake_variants: Vec<String>,
    pub sound_alikes: Vec<String>,
}

/// Compute cosine similarity between two vectors.
/// Returns a value between -1.0 and 1.0.
/// 1.0 = identical, 0.0 = orthogonal, -1.0 = opposite.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Verify a speaker embedding against an enrolled profile.
/// Returns the maximum cosine similarity across all enrolled vectors.
/// If similarity > threshold, the speaker is verified.
pub fn verify_speaker(profile: &VoiceProfile, embedding: &[f32]) -> f32 {
    if !profile.is_enrolled() {
        // No profile enrolled — accept all (verification disabled)
        return 1.0;
    }
    // Compute max similarity across all enrolled vectors
    // Use NEG_INFINITY as initial value so negative similarities are preserved
    let max_sim = profile
        .embeddings
        .iter()
        .map(|v| cosine_similarity(v, embedding))
        .fold(f32::NEG_INFINITY, f32::max);
    max_sim
}

/// Resolve the voice profile file path from the app data directory.
pub fn resolve_profile_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("voice_profile.json")
}

/// Speaker verifier using the OWW embedding model.
/// Extracts embeddings from audio and compares to enrolled profile.
pub struct SpeakerVerifier {
    /// Path to the voice profile JSON file
    profile_path: PathBuf,
    /// Loaded voice profile (if enrolled)
    profile: Option<VoiceProfile>,
}

impl SpeakerVerifier {
    /// Create a new speaker verifier.
    /// The profile is loaded from disk if it exists.
    pub fn new(profile_path: PathBuf) -> anyhow::Result<Self> {
        let profile = if profile_path.exists() {
            Some(VoiceProfile::load(&profile_path)?)
        } else {
            None
        };
        Ok(SpeakerVerifier {
            profile_path,
            profile,
        })
    }

    /// Get the loaded voice profile (if any).
    pub fn profile(&self) -> Option<&VoiceProfile> {
        self.profile.as_ref()
    }

    /// Check if a voice profile is enrolled.
    pub fn is_enrolled(&self) -> bool {
        self.profile.as_ref().map(|p| p.is_enrolled()).unwrap_or(false)
    }

    /// Enroll a speaker using the provided embedding vectors.
    /// Each embedding is extracted from an enrollment clip.
    pub fn enroll(
        &mut self,
        embeddings: Vec<Vec<f32>>,
        threshold: f32,
        wake_variants: Vec<String>,
    ) -> anyhow::Result<()> {
        if embeddings.len() < MIN_ENROLLMENT_CLIPS {
            return Err(anyhow::anyhow!(
                "Need at least {} enrollment clips, got {}",
                MIN_ENROLLMENT_CLIPS,
                embeddings.len()
            ));
        }

        let mut profile = VoiceProfile::new(threshold);
        for embedding in embeddings {
            profile.add_embedding(embedding);
        }
        profile.num_clips = profile.embeddings.len();
        if !wake_variants.is_empty() {
            profile.wake_variants = wake_variants;
        }
        profile.save(&self.profile_path)?;
        self.profile = Some(profile);
        Ok(())
    }

    /// Verify a speaker embedding against the enrolled profile.
    /// Returns (similarity, is_verified).
    /// If no profile is enrolled, returns (1.0, true) — accept all.
    pub fn verify(&self, embedding: &[f32]) -> (f32, bool) {
        match &self.profile {
            None => (1.0, true), // No profile — accept all
            Some(profile) => {
                let sim = verify_speaker(profile, embedding);
                (sim, sim >= profile.threshold)
            }
        }
    }

    /// Add an accepted embedding to the profile (implicit enrollment).
    /// This grows the profile up to MAX_ENROLLMENT_VECTORS.
    pub fn implicit_enroll(&mut self, embedding: Vec<f32>) -> anyhow::Result<()> {
        if let Some(profile) = &mut self.profile {
            profile.add_embedding(embedding);
            profile.save(&self.profile_path)?;
        }
        Ok(())
    }

    /// Delete the voice profile.
    pub fn delete(&mut self) -> anyhow::Result<()> {
        if self.profile_path.exists() {
            std::fs::remove_file(&self.profile_path)?;
        }
        self.profile = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.001, "Identical vectors should have sim=1.0, got {sim}");
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.001, "Orthogonal vectors should have sim=0.0, got {sim}");
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0];
        let b = vec![-1.0, -2.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 0.001, "Opposite vectors should have sim=-1.0, got {sim}");
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0, "Empty vectors should return 0.0");
    }

    #[test]
    fn test_voice_profile_new() {
        let profile = VoiceProfile::new(DEFAULT_THRESHOLD);
        assert!(!profile.is_enrolled());
        assert_eq!(profile.num_embeddings(), 0);
        assert_eq!(profile.threshold, DEFAULT_THRESHOLD);
        assert_eq!(profile.wake_variants, vec!["nexus".to_string()]);
    }

    #[test]
    fn test_voice_profile_add_embedding() {
        let mut profile = VoiceProfile::new(DEFAULT_THRESHOLD);
        let embedding = vec![0.1; 96];
        profile.add_embedding(embedding);
        assert!(profile.is_enrolled());
        assert_eq!(profile.num_embeddings(), 1);
    }

    #[test]
    fn test_voice_profile_max_embeddings() {
        let mut profile = VoiceProfile::new(DEFAULT_THRESHOLD);
        for i in 0..MAX_ENROLLMENT_VECTORS + 5 {
            profile.add_embedding(vec![i as f32; 96]);
        }
        assert_eq!(profile.num_embeddings(), MAX_ENROLLMENT_VECTORS);
    }

    #[test]
    fn test_verify_speaker_no_profile() {
        let profile = VoiceProfile::new(DEFAULT_THRESHOLD);
        let embedding = vec![0.5; 96];
        let sim = verify_speaker(&profile, &embedding);
        assert_eq!(sim, 1.0, "No profile should accept all (sim=1.0)");
    }

    #[test]
    fn test_verify_speaker_enrolled() {
        let mut profile = VoiceProfile::new(DEFAULT_THRESHOLD);
        let enrolled_embedding = vec![0.5; 96];
        profile.add_embedding(enrolled_embedding);

        // Same embedding → high similarity
        let test_embedding = vec![0.5; 96];
        let sim = verify_speaker(&profile, &test_embedding);
        assert!(sim > 0.99, "Same embedding should have sim > 0.99, got {sim}");

        // Different embedding → lower similarity
        let test_embedding = vec![-0.5; 96];
        let sim = verify_speaker(&profile, &test_embedding);
        assert!(sim < 0.0, "Opposite embedding should have sim < 0.0, got {sim}");
    }

    #[test]
    fn test_speaker_verifier_enroll_and_verify() {
        let temp_dir = std::env::temp_dir().join("nexus_voice_profile_test");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let profile_path = temp_dir.join("voice_profile.json");

        // Clean up any existing profile
        let _ = std::fs::remove_file(&profile_path);

        let mut verifier = SpeakerVerifier::new(profile_path.clone()).unwrap();
        assert!(!verifier.is_enrolled());

        // Enroll with 3 embeddings
        let embeddings = vec![
            vec![0.5; 96],
            vec![0.51; 96],
            vec![0.49; 96],
        ];
        verifier
            .enroll(embeddings, DEFAULT_THRESHOLD, vec!["nexus".to_string()])
            .unwrap();
        assert!(verifier.is_enrolled());

        // Verify with similar embedding → should pass
        let test_embedding = vec![0.5; 96];
        let (sim, verified) = verifier.verify(&test_embedding);
        assert!(verified, "Similar embedding should be verified (sim={sim})");
        assert!(sim > DEFAULT_THRESHOLD, "Similar embedding should have sim > threshold");

        // Verify with different embedding → should fail
        let test_embedding = vec![-0.5; 96];
        let (sim, verified) = verifier.verify(&test_embedding);
        assert!(!verified, "Different embedding should not be verified (sim={sim})");
        assert!(sim < DEFAULT_THRESHOLD, "Different embedding should have sim < threshold");

        // Clean up
        let _ = std::fs::remove_file(&profile_path);
    }

    #[test]
    fn test_speaker_verifier_implicit_enroll() {
        let temp_dir = std::env::temp_dir().join("nexus_voice_profile_test2");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let profile_path = temp_dir.join("voice_profile.json");
        let _ = std::fs::remove_file(&profile_path);

        let mut verifier = SpeakerVerifier::new(profile_path.clone()).unwrap();

        // Enroll with 3 embeddings
        verifier
            .enroll(
                vec![vec![0.5; 96], vec![0.51; 96], vec![0.49; 96]],
                DEFAULT_THRESHOLD,
                vec!["nexus".to_string()],
            )
            .unwrap();
        assert_eq!(verifier.profile().unwrap().num_embeddings(), 3);

        // Implicit enroll adds one more
        verifier.implicit_enroll(vec![0.52; 96]).unwrap();
        assert_eq!(verifier.profile().unwrap().num_embeddings(), 4);

        // Clean up
        let _ = std::fs::remove_file(&profile_path);
    }

    #[test]
    fn test_speaker_verifier_delete() {
        let temp_dir = std::env::temp_dir().join("nexus_voice_profile_test3");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let profile_path = temp_dir.join("voice_profile.json");
        let _ = std::fs::remove_file(&profile_path);

        let mut verifier = SpeakerVerifier::new(profile_path.clone()).unwrap();
        verifier
            .enroll(
                vec![vec![0.5; 96], vec![0.51; 96], vec![0.49; 96]],
                DEFAULT_THRESHOLD,
                vec!["nexus".to_string()],
            )
            .unwrap();
        assert!(verifier.is_enrolled());
        assert!(profile_path.exists());

        verifier.delete().unwrap();
        assert!(!verifier.is_enrolled());
        assert!(!profile_path.exists());
    }

    #[test]
    fn test_voice_profile_save_load() {
        let temp_dir = std::env::temp_dir().join("nexus_voice_profile_test4");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let profile_path = temp_dir.join("voice_profile.json");
        let _ = std::fs::remove_file(&profile_path);

        let mut profile = VoiceProfile::new(0.5);
        profile.add_embedding(vec![0.1; 96]);
        profile.add_embedding(vec![0.2; 96]);
        profile.wake_variants = vec!["nexus".to_string(), "hey nexus".to_string()];
        profile.save(&profile_path).unwrap();

        let loaded = VoiceProfile::load(&profile_path).unwrap();
        assert_eq!(loaded.num_embeddings(), 2);
        assert_eq!(loaded.threshold, 0.5);
        assert_eq!(loaded.wake_variants, vec!["nexus".to_string(), "hey nexus".to_string()]);

        let _ = std::fs::remove_file(&profile_path);
    }
}
