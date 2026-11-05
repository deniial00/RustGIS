use super::*;

#[path = "../../../tests/support/ogc_document_logins.rs"]
mod documents;

const BASE: &str = "https://maps.example.org/folder/caps.xml";
const ORIGIN: &str = "https://maps.example.org";
const BASE_BINDING: &str = "0123456789abcdef";
const FOREIGN_BINDING: &str = "fedcba9876543210";

fn requests(service: &Service) -> Vec<String> {
    assert_eq!(service.layers.len(), 1);
    let layer = &service.layers[0];
    let mut requests = match &layer.offer {
        Offer::Tiles { template } => vec![template.clone()],
        Offer::Features { url } => vec![url.clone()],
        Offer::Coverage(c) => vec![
            describe_coverage_url(c),
            get_coverage_url(c, &CoverageInfo { format: "image/tiff".into(), axes: None, bbox: None }, [1.0, 2.0, 3.0, 4.0]),
        ],
        other => panic!("unexpected offer: {other:?}"),
    };
    if let Some(info) = &layer.feature_info {
        assert!(info.queryable(), "trusted inherited wrappers remain usable internally");
        requests.push(info.request(0.0, 0.0, 1.0).unwrap());
    }
    requests.extend(layer.legends.iter().map(|legend| legend.url.clone()));
    requests
}

fn assert_authority(request: &str, binding: Option<&str>, target: &str, context: &str) {
    let bound = crate::login_address::parse(request).unwrap();
    assert_eq!(bound.map(|b| b.binding), binding, "{context}: {request}");
    if let Some(bound) = bound {
        assert_eq!(bound.origin, ORIGIN, "{context}");
    }
    let inner = crate::login_address::inner(request);
    assert!(!inner.contains(crate::login_address::PREFIX), "{context}: nested runtime authority");
    assert!(inner.starts_with(target), "{context}: {inner} should target {target}");
}

#[test]
fn document_endpoints_inherit_only_the_trusted_base_login() {
    let bound_base = crate::login_address::bind(BASE_BINDING, ORIGIN, BASE).unwrap();
    let foreign =
        crate::login_address::bind(FOREIGN_BINDING, "https://foreign.example.org", "https://foreign.example.org/advertised").unwrap();
    let same_origin_foreign = crate::login_address::bind(FOREIGN_BINDING, ORIGIN, "https://maps.example.org/advertised").unwrap();
    for (href, target, same_origin) in [
        ("https://maps.example.org/advertised", "https://maps.example.org/advertised", true),
        ("https://MAPS.example.org:443/advertised", "https://MAPS.example.org:443/advertised", true),
        ("http://maps.example.org/advertised", "http://maps.example.org/advertised", false),
        ("https://maps.example.org:444/advertised", "https://maps.example.org:444/advertised", false),
        ("https://foreign.example.org/advertised", "https://foreign.example.org/advertised", false),
        ("//foreign.example.org/advertised", "https://foreign.example.org/advertised", false),
        ("/advertised", "https://maps.example.org/advertised", true),
        ("advertised", "https://maps.example.org/folder/advertised", true),
        (foreign.as_str(), "https://foreign.example.org/advertised", false),
        (same_origin_foreign.as_str(), "https://maps.example.org/advertised", true),
    ] {
        for (base, has_login) in [(BASE, false), (bound_base.as_str(), true)] {
            for (boundary, document) in documents::documents(href) {
                let service = parse(base, document.as_bytes()).unwrap();
                let expected = (has_login && same_origin).then_some(BASE_BINDING);
                let requests = requests(&service);
                for request in &requests {
                    assert_authority(request, expected, target, boundary);
                }
                let inner = crate::login_address::inner(&requests[0]);
                match &service.layers[0].offer {
                    Offer::Tiles { .. } if boundary == "WMTS ResourceURL" => {
                        assert_eq!(inner, format!("{target}/{{z}}/{{y}}/{{x}}.png"));
                    }
                    Offer::Tiles { .. } => {
                        assert!(inner.contains(if service.kind == "WMS" { "REQUEST=GetMap" } else { "REQUEST=GetTile" }));
                    }
                    Offer::Features { .. } => assert!(inner.contains("REQUEST=GetFeature")),
                    Offer::Coverage(_) => assert!(inner.contains("REQUEST=DescribeCoverage")),
                    other => panic!("unexpected offer: {other:?}"),
                }
            }
        }
    }
}

#[test]
fn malformed_document_wrappers_cannot_supply_authority() {
    let base = crate::login_address::bind(BASE_BINDING, ORIGIN, BASE).unwrap();
    for bad in [
        "rustgis-login:secret",
        "rustgis-login:fedcba9876543210:1:é:https://foreign.example.org/advertised",
        "rustgis-login:fedcba9876543210:0::https://foreign.example.org/advertised",
        "httpfoo://foreign.example.org/advertised",
        "file:///etc/passwd",
    ] {
        for (boundary, document) in documents::documents(bad) {
            let service = parse(&base, document.as_bytes()).unwrap();
            for request in requests(&service) {
                assert_authority(&request, Some(BASE_BINDING), BASE, boundary);
            }
        }
    }
}

#[test]
fn oversized_same_origin_document_targets_fail_closed() {
    let base = crate::login_address::bind(BASE_BINDING, ORIGIN, BASE).unwrap();
    let target = format!("{ORIGIN}/{}", "a".repeat(17000));
    let prefix = format!("rustgis-login:{BASE_BINDING}:{}:{ORIGIN}:", ORIGIN.len());
    for (boundary, document) in documents::documents(&target) {
        let service = parse(&base, document.as_bytes()).unwrap();
        assert_eq!(service.layers.len(), 1, "{boundary}");
        let requests = match &service.layers[0].offer {
            Offer::Tiles { template } => vec![template.clone()],
            Offer::Features { url } => vec![url.clone()],
            Offer::Coverage(c) => vec![
                describe_coverage_url(c),
                get_coverage_url(c, &CoverageInfo { format: "image/tiff".into(), axes: None, bbox: None }, [1.0, 2.0, 3.0, 4.0]),
            ],
            other => panic!("unexpected offer: {other:?}"),
        };
        for request in requests {
            assert!(request.starts_with(&prefix), "{boundary}: must not downgrade to anonymous");
            assert!(request.contains(&target), "{boundary}: must not replace the target with the base endpoint");
            assert!(crate::login_address::parse(&request).is_err(), "{boundary}");
            assert!(crate::login_address::check(&request).is_err(), "{boundary}");
        }
    }
}
