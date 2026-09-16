mod memory;
mod postgres;

pub(crate) use aether_data_contracts::repository::routing_profiles::{
    CreateRoutingGroupBindingRecord, CreateRoutingGroupRecord, CreateRoutingGroupVersionRecord,
    RoutingGroupBindingQuery, RoutingGroupBindingSubject, RoutingGroupLookupKey,
    RoutingGroupReadRepository, RoutingGroupWriteRepository, StoredRoutingGroup,
    StoredRoutingGroupBinding, StoredRoutingGroupVersion, UpdateRoutingGroupBindingRecord,
    UpdateRoutingGroupRecord,
};
pub use memory::InMemoryRoutingGroupRepository;
pub use postgres::PostgresRoutingGroupRepository;
