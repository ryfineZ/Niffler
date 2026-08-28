mod memory;
mod postgres;
mod types;

pub use memory::InMemoryOAuthProviderRepository;
pub use postgres::SqlxOAuthProviderRepository;
pub use types::{
    EncryptedSecretUpdate, OAuthProviderReadRepository, OAuthProviderRepository,
    OAuthProviderWriteRepository, StoredOAuthProviderConfig, UpsertOAuthProviderConfigRecord,
};
