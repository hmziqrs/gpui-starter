use gpui::TestAppContext;

use crate::client::QueryClient;
use crate::core::*;
use crate::integration_client_fixtures::*;

#[gpui::test]
fn client_cancel_query_cancels_active_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "42"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Start a request
        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let result = r.begin_request(sequencer, 1_000, QueryFetchMode::Normal);
            assert!(matches!(result, QueryBeginResult::Started { .. }));
        });

        // Grab the signal before cancelling
        let signal = client.signal_for::<User, QueryError>(&key, cx);
        assert!(signal.is_some(), "signal should exist while loading");
        let signal = signal.unwrap();
        assert!(!signal.is_cancelled());

        // Cancel via client
        let cancelled =
            client.cancel_query::<User, QueryError>(&key, QueryError::cancelled("aborted"), cx);
        assert!(cancelled, "should have cancelled an active request");
        assert_eq!(entity.read(cx).status(), QueryStatus::Cancelled);
        assert!(signal.is_cancelled(), "signal should be cancelled");
    });
}

#[gpui::test]
fn client_cancel_query_returns_false_for_idle_resource(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "99"]);
        let _entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Resource is idle (no request started)
        let cancelled =
            client.cancel_query::<User, QueryError>(&key, QueryError::cancelled("nope"), cx);
        assert!(!cancelled, "idle resource should not be cancellable");
    });
}

#[gpui::test]
fn client_cancel_query_returns_false_for_nonexistent_key(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let cancelled = client.cancel_query::<User, QueryError>(
            &QueryKey::from("ghost"),
            QueryError::cancelled("nope"),
            cx,
        );
        assert!(!cancelled, "nonexistent key should not be cancellable");
    });
}

#[gpui::test]
fn client_signal_for_returns_none_when_no_active_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "1"]);
        let _entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Resource is idle, no signal
        let signal = client.signal_for::<User, QueryError>(&key, cx);
        assert!(signal.is_none(), "no signal for idle resource");
    });
}

#[gpui::test]
fn client_signal_for_returns_signal_while_loading(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "7"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let _ = r.begin_request(sequencer, 1_000, QueryFetchMode::Normal);
        });

        let signal = client.signal_for::<User, QueryError>(&key, cx);
        assert!(signal.is_some(), "signal should exist while loading");
        assert!(!signal.unwrap().is_cancelled());
    });
}
