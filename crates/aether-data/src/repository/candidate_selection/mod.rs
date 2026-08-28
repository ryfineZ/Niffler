mod memory;
mod postgres;

#[allow(unused_imports)]
pub(crate) use aether_data_contracts::repository::candidate_selection::{
    MinimalCandidateSelectionReadRepository, MinimalCandidateSelectionRepository,
    StoredMinimalCandidateSelectionRow, StoredPoolKeyCandidateOrder,
    StoredPoolKeyCandidateRowsByKeyIdsQuery, StoredPoolKeyCandidateRowsQuery,
    StoredProviderModelMapping, StoredRequestedModelCandidateRowsQuery,
};
pub use memory::InMemoryMinimalCandidateSelectionReadRepository;
pub use postgres::SqlxMinimalCandidateSelectionReadRepository;
