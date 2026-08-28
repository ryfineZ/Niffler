mod admission;
mod memory;
mod postgres;

#[allow(unused_imports)]
pub(crate) use aether_data_contracts::repository::candidates::{
    build_decision_trace, derive_request_candidate_final_status, DecisionTrace,
    DecisionTraceCandidate, PublicHealthStatusCount, PublicHealthTimelineBucket,
    RequestCandidateFinalStatus, RequestCandidateReadRepository, RequestCandidateRepository,
    RequestCandidateStatus, RequestCandidateTrace, RequestCandidateWriteRepository,
    StoredRequestCandidate, UpsertRequestCandidateRecord,
};
pub use memory::InMemoryRequestCandidateRepository;
pub use postgres::SqlxRequestCandidateReadRepository;
