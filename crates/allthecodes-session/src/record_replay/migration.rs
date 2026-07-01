#[derive(Debug, Clone, Default)]
pub struct LegacyMigrationReport {
    pub session_id: String,
    pub migrated_messages: usize,
    pub legacy_messages: usize,
}
