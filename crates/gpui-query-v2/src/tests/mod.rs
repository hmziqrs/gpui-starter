mod test_support;
mod core_cache;
mod core_lifecycle;
mod core_mutation;
mod core_infinite_query;
mod core_request;
mod core_select;
mod core_policy_types;
mod core_resource_advanced;
mod coverage_gaps;
mod integration_client;
#[cfg(feature = "hook")]
mod integration_client_coverage;
#[cfg(feature = "hook")]
mod property_tests;
#[cfg(feature = "hook")]
mod hook_tests;
