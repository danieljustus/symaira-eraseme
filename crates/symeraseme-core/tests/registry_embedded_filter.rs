use symeraseme_core::registry::{BrokerFilter, filter_brokers, load_embedded};

#[test]
fn embedded_registry_is_complete_sorted_and_metadata_validated() {
    let brokers = load_embedded().expect("embedded registry");
    assert_eq!(brokers.len(), 1_277);
    assert_eq!(brokers.first().unwrap().id, "0ptimus-analytics-us");
    assert_eq!(brokers.last().unwrap().id, "zs-associates-us");
    assert!(brokers.windows(2).all(|pair| pair[0].id < pair[1].id));
    let mut samples = Vec::new();
    for _ in 0..3 {
        let started = std::time::Instant::now();
        let _ = load_embedded().unwrap();
        samples.push(started.elapsed().as_nanos());
    }
    eprintln!("embedded_load_samples_ns={samples:?}");
}

#[test]
fn filters_match_go_defaults_and_preserve_order() {
    let brokers = load_embedded().unwrap();
    let default = filter_brokers(&brokers, &BrokerFilter::default());
    assert_eq!(default.len(), 1_273);
    let empty_selectors = filter_brokers(
        &brokers,
        &BrokerFilter {
            jurisdiction: Some(String::new()),
            law: Some(String::new()),
            priority: Some(String::new()),
            category: Some(String::new()),
            status: Some(String::new()),
            ..BrokerFilter::default()
        },
    );
    assert_eq!(empty_selectors.len(), default.len());
    assert!(default.windows(2).all(|pair| pair[0].id < pair[1].id));

    let include_disabled = filter_brokers(
        &brokers,
        &BrokerFilter {
            include_disabled: true,
            ..BrokerFilter::default()
        },
    );
    assert_eq!(include_disabled.len(), 1_276);

    let inactive = filter_brokers(
        &brokers,
        &BrokerFilter {
            include_inactive: true,
            ..BrokerFilter::default()
        },
    );
    assert_eq!(inactive.len(), 1_274);

    let status_ignored = filter_brokers(
        &brokers,
        &BrokerFilter {
            status: Some("out-of-business".to_owned()),
            include_inactive: true,
            ..BrokerFilter::default()
        },
    );
    assert_eq!(status_ignored.len(), 1_274);

    let inactive_status = filter_brokers(
        &brokers,
        &BrokerFilter {
            status: Some("out-of-business".to_owned()),
            ..BrokerFilter::default()
        },
    );
    assert_eq!(inactive_status.len(), 1);

    let jurisdiction = filter_brokers(
        &brokers,
        &BrokerFilter {
            jurisdiction: Some("US".to_owned()),
            ..BrokerFilter::default()
        },
    );
    assert!(!jurisdiction.is_empty());
    assert!(jurisdiction.iter().all(|broker| {
        broker
            .jurisdictions
            .iter()
            .any(|value| serde_json::to_value(value).unwrap() == "US")
    }));
}
