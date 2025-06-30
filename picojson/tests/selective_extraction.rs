// Demo of selective extraction of data from a JSON document.
use picojson::{Event, PullParser};

// A more complex, "real-world" JSON document
const REAL_WORLD_JSON: &str = r#"
{
    "user_id": "u-12345",
    "username": "jdoe",
    "email": "jdoe@example.com",
    "is_active": true,
    "feature_flags": {
        "new_dashboard": true,
        "beta_access": false,
        "experimental_api": null
    },
    "products": [
        {
            "product_id": "p-001",
            "name": "Widget A",
            "stock": 99,
            "tags": ["gadget", "tech"]
        },
        {
            "product_id": "p-002",
            "name": "Widget B",
            "stock": 150,
            "tags": ["gadget", "classic"]
        },
        {
            "product_id": "p-003",
            "name": "Widget C",
            "stock": 42,
            "tags": ["new", "tech"]
        }
    ],
    "metadata": {
        "last_login": "2025-06-29T10:00:00Z",
        "notes": "A string with an escape sequence \n here."
    }
}
"#;

// State for our selective extraction logic
#[derive(Default)]
struct ExtractedData {
    email: Option<std::string::String>,
    second_product_id: Option<std::string::String>,
    new_dashboard_status: Option<bool>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum ExtractionState {
    Idle,
    InProductsArray,
    InFeatureFlags,
    ExpectingEmail,
    ExpectingProductId,
    ExpectingNewDashboardStatus,
}

#[test]
fn test_selective_extraction() {
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::with_buffer(REAL_WORLD_JSON, &mut scratch);

    let mut extracted = ExtractedData::default();
    let mut state = ExtractionState::Idle;
    let mut product_index = 0;
    let mut array_depth = 0;

    loop {
        let event = match parser.next_event() {
            Ok(Event::EndDocument) => break,
            Ok(event) => event,
            Err(e) => panic!("Parse error: {:?}", e),
        };

        match event {
            Event::Key(key) => {
                state = match (state, &*key) {
                    (_, "email") => ExtractionState::ExpectingEmail,
                    (_, "products") => ExtractionState::InProductsArray,
                    (ExtractionState::InProductsArray, "product_id") => {
                        ExtractionState::ExpectingProductId
                    }
                    (_, "feature_flags") => ExtractionState::InFeatureFlags,
                    (ExtractionState::InFeatureFlags, "new_dashboard") => {
                        ExtractionState::ExpectingNewDashboardStatus
                    }
                    (current_state, _) => current_state,
                };
            }
            Event::String(s) => match (state, product_index) {
                (ExtractionState::ExpectingEmail, _) => {
                    extracted.email = Some(s.as_ref().to_owned());
                    state = ExtractionState::Idle;
                }
                (ExtractionState::ExpectingProductId, 1) => {
                    extracted.second_product_id = Some(s.as_ref().to_owned());
                    state = ExtractionState::InProductsArray;
                }
                (ExtractionState::ExpectingProductId, _) => {
                    state = ExtractionState::InProductsArray;
                }
                _ => {}
            },
            Event::Bool(b) => match state {
                ExtractionState::ExpectingNewDashboardStatus => {
                    extracted.new_dashboard_status = Some(b);
                    state = ExtractionState::InFeatureFlags;
                }
                _ => {}
            },
            Event::StartArray => {
                if state == ExtractionState::InProductsArray {
                    array_depth += 1;
                    if array_depth == 1 {
                        product_index = 0;
                    }
                }
            }
            Event::EndObject => match state {
                ExtractionState::InProductsArray => {
                    product_index += 1;
                }
                ExtractionState::InFeatureFlags => {
                    state = ExtractionState::Idle;
                }
                _ => {}
            },
            Event::EndArray => {
                if state == ExtractionState::InProductsArray {
                    array_depth -= 1;
                    if array_depth == 0 {
                        state = ExtractionState::Idle;
                    }
                }
            }
            _ => {}
        }
    }

    // Assert that we extracted the correct data
    assert_eq!(extracted.email, Some("jdoe@example.com".to_string()));
    assert_eq!(extracted.second_product_id, Some("p-002".to_string()));
    assert_eq!(extracted.new_dashboard_status, Some(true));
}
