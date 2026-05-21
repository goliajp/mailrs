use serde::{Deserialize, Serialize};

/// Structured data extracted from email HTML (Schema.org JSON-LD).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StructuredData {
    /// Travel / hotel / restaurant reservations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reservations: Vec<Reservation>,
    /// E-commerce orders / receipts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub orders: Vec<Order>,
    /// Events the message is RSVP'ing to or announcing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventInfo>,
    /// Schema.org Action handles (`confirm` / `view` / `track` / `rsvp` URLs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionInfo>,
}

/// One reservation (flight, hotel, restaurant, rental car, etc).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    /// Reservation kind: `flight` / `hotel` / `restaurant` / `rental_car`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Display name (hotel name, restaurant name, etc).
    pub name: Option<String>,
    /// Reservation confirmation code.
    pub reservation_id: Option<String>,
    /// Status string (`confirmed` / `cancelled` / `pending`).
    pub status: Option<String>,
    /// ISO-8601 start date / time.
    pub start_date: Option<String>,
    /// ISO-8601 end date / time.
    pub end_date: Option<String>,
    /// Location (address / city / airport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Provider / brand (airline, hotel chain, etc).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// IATA code or name of the departure airport (flight only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub departure_airport: Option<String>,
    /// IATA code or name of the arrival airport (flight only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_airport: Option<String>,
    /// Flight number (flight only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_number: Option<String>,
}

/// One e-commerce order / receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Merchant-assigned order number.
    pub order_number: Option<String>,
    /// Merchant / seller name.
    pub merchant: Option<String>,
    /// ISO-8601 date the order was placed.
    pub order_date: Option<String>,
    /// Status string (`placed` / `shipped` / `delivered` / ...).
    pub status: Option<String>,
    /// Line items.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<OrderItem>,
    /// Total amount as a string (no parsing — keep original currency formatting).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    /// Currency code (ISO 4217).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// One line item inside an [`Order`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    /// Item name.
    pub name: String,
    /// Quantity ordered, if specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<u32>,
    /// Per-item price as a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
}

/// One event referenced in the message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventInfo {
    /// Event name.
    pub name: String,
    /// ISO-8601 start date / time.
    pub start_date: Option<String>,
    /// ISO-8601 end date / time.
    pub end_date: Option<String>,
    /// Location string.
    pub location: Option<String>,
    /// Detail URL (registration page, etc).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Schema.org Action handle ("View Booking", "Track Package", ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Action kind (`confirm` / `view` / `track` / `rsvp`).
    #[serde(rename = "type")]
    pub kind: String,
    /// Action display name.
    pub name: String,
    /// Target URL.
    pub url: Option<String>,
}

impl StructuredData {
    /// `true` when no structured data was extracted from the message.
    pub fn is_empty(&self) -> bool {
        self.reservations.is_empty()
            && self.orders.is_empty()
            && self.events.is_empty()
            && self.actions.is_empty()
    }
}

/// extract Schema.org JSON-LD from HTML email body
pub fn extract_structured_data(html: &str) -> StructuredData {
    let mut data = StructuredData::default();

    // find all <script type="application/ld+json"> blocks
    for block in extract_jsonld_blocks(html) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&block) else {
            continue;
        };

        // handle single object or @graph array
        let items = if let Some(graph) = value.get("@graph").and_then(|g| g.as_array()) {
            graph.clone()
        } else if value.is_array() {
            value.as_array().cloned().unwrap_or_default()
        } else {
            vec![value]
        };

        for item in items {
            process_jsonld_item(&item, &mut data);
        }
    }

    data
}

fn extract_jsonld_blocks(html: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let lower = html.to_lowercase();
    let mut search_from = 0;

    while let Some(tag_start) = lower[search_from..].find("application/ld+json") {
        let abs_tag = search_from + tag_start;

        // find the end of the opening <script> tag
        let Some(open_end) = lower[abs_tag..].find('>') else {
            break;
        };
        let content_start = abs_tag + open_end + 1;

        // find closing </script>
        let Some(close_pos) = lower[content_start..].find("</script>") else {
            break;
        };
        let content_end = content_start + close_pos;

        let content = html[content_start..content_end].trim();
        if !content.is_empty() {
            blocks.push(content.to_string());
        }

        search_from = content_end + 9;
    }

    blocks
}

fn process_jsonld_item(item: &serde_json::Value, data: &mut StructuredData) {
    let schema_type = item
        .get("@type")
        .and_then(|t| t.as_str())
        .unwrap_or("");

    match schema_type {
        "FlightReservation" => {
            let flight = item.get("reservationFor");
            data.reservations.push(Reservation {
                kind: "flight".into(),
                name: get_str(flight, "name"),
                reservation_id: get_str(Some(item), "reservationId"),
                status: get_str(Some(item), "reservationStatus")
                    .map(|s| s.replace("http://schema.org/Reservation", "")),
                start_date: get_str(flight, "departureTime"),
                end_date: get_str(flight, "arrivalTime"),
                location: None,
                provider: get_nested_str(item, &["provider", "name"])
                    .or_else(|| get_nested_str(item, &["reservationFor", "airline", "name"])),
                departure_airport: get_nested_str(item, &["reservationFor", "departureAirport", "iataCode"])
                    .or_else(|| get_nested_str(item, &["reservationFor", "departureAirport", "name"])),
                arrival_airport: get_nested_str(item, &["reservationFor", "arrivalAirport", "iataCode"])
                    .or_else(|| get_nested_str(item, &["reservationFor", "arrivalAirport", "name"])),
                flight_number: get_str(flight, "flightNumber"),
            });
        }
        "LodgingReservation" => {
            data.reservations.push(Reservation {
                kind: "hotel".into(),
                name: get_nested_str(item, &["reservationFor", "name"]),
                reservation_id: get_str(Some(item), "reservationId"),
                status: get_str(Some(item), "reservationStatus"),
                start_date: get_str(Some(item), "checkinTime"),
                end_date: get_str(Some(item), "checkoutTime"),
                location: get_nested_str(item, &["reservationFor", "address", "streetAddress"]),
                provider: get_nested_str(item, &["reservationFor", "name"]),
                departure_airport: None,
                arrival_airport: None,
                flight_number: None,
            });
        }
        "FoodEstablishmentReservation" => {
            data.reservations.push(Reservation {
                kind: "restaurant".into(),
                name: get_nested_str(item, &["reservationFor", "name"]),
                reservation_id: get_str(Some(item), "reservationId"),
                status: get_str(Some(item), "reservationStatus"),
                start_date: get_str(Some(item), "startTime"),
                end_date: get_str(Some(item), "endTime"),
                location: get_nested_str(item, &["reservationFor", "address", "streetAddress"]),
                provider: None,
                departure_airport: None,
                arrival_airport: None,
                flight_number: None,
            });
        }
        "RentalCarReservation" => {
            data.reservations.push(Reservation {
                kind: "rental_car".into(),
                name: get_nested_str(item, &["reservationFor", "name"]),
                reservation_id: get_str(Some(item), "reservationId"),
                status: get_str(Some(item), "reservationStatus"),
                start_date: get_str(Some(item), "pickupTime"),
                end_date: get_str(Some(item), "dropoffTime"),
                location: get_nested_str(item, &["pickupLocation", "name"]),
                provider: get_nested_str(item, &["provider", "name"]),
                departure_airport: None,
                arrival_airport: None,
                flight_number: None,
            });
        }
        "Order" => {
            let items = item
                .get("orderedItem")
                .or_else(|| item.get("acceptedOffer"))
                .and_then(|v| {
                    if v.is_array() {
                        v.as_array().cloned()
                    } else {
                        Some(vec![v.clone()])
                    }
                })
                .unwrap_or_default()
                .iter()
                .filter_map(|oi| {
                    let name = get_str(Some(oi), "name")
                        .or_else(|| get_nested_str(oi, &["orderedItem", "name"]))
                        .unwrap_or_default();
                    if name.is_empty() {
                        return None;
                    }
                    Some(OrderItem {
                        name,
                        quantity: oi.get("orderQuantity").and_then(|q| q.as_u64()).map(|q| q as u32),
                        price: get_str(Some(oi), "price"),
                    })
                })
                .collect();

            data.orders.push(Order {
                order_number: get_str(Some(item), "orderNumber"),
                merchant: get_nested_str(item, &["seller", "name"])
                    .or_else(|| get_nested_str(item, &["merchant", "name"])),
                order_date: get_str(Some(item), "orderDate"),
                status: get_str(Some(item), "orderStatus")
                    .map(|s| s.replace("http://schema.org/Order", "")),
                items,
                total: get_nested_str(item, &["totalPrice", "value"])
                    .or_else(|| get_str(Some(item), "totalPrice").and_then(|v| {
                        if v.parse::<f64>().is_ok() { Some(v) } else { None }
                    })),
                currency: get_nested_str(item, &["totalPrice", "currency"])
                    .or_else(|| get_str(Some(item), "priceCurrency")),
            });
        }
        "Event" | "MusicEvent" | "SportsEvent" | "BusinessEvent" | "SocialEvent" => {
            data.events.push(EventInfo {
                name: get_str(Some(item), "name").unwrap_or_default(),
                start_date: get_str(Some(item), "startDate"),
                end_date: get_str(Some(item), "endDate"),
                location: get_nested_str(item, &["location", "name"])
                    .or_else(|| get_str(Some(item), "location").filter(|s| !s.starts_with('{'))),
                url: sanitize_url(get_str(Some(item), "url")),
            });
        }
        "ConfirmAction" | "ViewAction" | "TrackAction" | "RsvpAction" | "SaveAction" => {
            let kind = schema_type.replace("Action", "").to_lowercase();
            data.actions.push(ActionInfo {
                kind,
                name: get_str(Some(item), "name").unwrap_or_else(|| schema_type.into()),
                url: sanitize_url(
                    get_nested_str(item, &["target", "urlTemplate"])
                        .or_else(|| get_str(Some(item), "url")),
                ),
            });
        }
        // recurse into potential actions on other types
        _ => {}
    }

    // extract potentialAction from any item
    if let Some(actions) = item.get("potentialAction") {
        let action_list = if actions.is_array() {
            actions.as_array().cloned().unwrap_or_default()
        } else {
            vec![actions.clone()]
        };
        for action in action_list {
            let action_type = action.get("@type").and_then(|t| t.as_str()).unwrap_or("");
            if matches!(
                action_type,
                "ConfirmAction" | "ViewAction" | "TrackAction" | "RsvpAction" | "SaveAction"
            ) {
                process_jsonld_item(&action, data);
            }
        }
    }
}

fn get_str(obj: Option<&serde_json::Value>, key: &str) -> Option<String> {
    obj?.get(key)?.as_str().map(|s| s.to_string())
}

fn get_nested_str(obj: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = obj;
    for &key in path {
        current = current.get(key)?;
    }
    current.as_str().map(|s| s.to_string())
}

/// filter URL to only allow http/https protocols
fn sanitize_url(url: Option<String>) -> Option<String> {
    url.filter(|u| u.starts_with("http://") || u.starts_with("https://"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_flight_reservation() {
        let html = r#"<html><head>
        <script type="application/ld+json">
        {
            "@context": "http://schema.org",
            "@type": "FlightReservation",
            "reservationId": "RXJ34P",
            "reservationStatus": "http://schema.org/ReservationConfirmed",
            "reservationFor": {
                "@type": "Flight",
                "flightNumber": "110",
                "airline": {"@type": "Airline", "name": "United Airlines", "iataCode": "UA"},
                "departureAirport": {"@type": "Airport", "name": "San Francisco", "iataCode": "SFO"},
                "arrivalAirport": {"@type": "Airport", "name": "Los Angeles", "iataCode": "LAX"},
                "departureTime": "2026-04-01T08:00:00-07:00",
                "arrivalTime": "2026-04-01T09:30:00-07:00"
            }
        }
        </script></head><body>Your flight is confirmed.</body></html>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.reservations.len(), 1);
        let r = &data.reservations[0];
        assert_eq!(r.kind, "flight");
        assert_eq!(r.reservation_id.as_deref(), Some("RXJ34P"));
        assert_eq!(r.departure_airport.as_deref(), Some("SFO"));
        assert_eq!(r.arrival_airport.as_deref(), Some("LAX"));
        assert_eq!(r.flight_number.as_deref(), Some("110"));
    }

    #[test]
    fn extract_hotel_reservation() {
        let html = r#"<html><body>
        <script type="application/ld+json">
        {
            "@type": "LodgingReservation",
            "reservationId": "HTL-789",
            "checkinTime": "2026-05-01T15:00:00",
            "checkoutTime": "2026-05-03T11:00:00",
            "reservationFor": {
                "@type": "Hotel",
                "name": "Grand Hyatt Tokyo",
                "address": {"streetAddress": "6-10-3 Roppongi"}
            }
        }
        </script></body></html>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.reservations.len(), 1);
        let r = &data.reservations[0];
        assert_eq!(r.kind, "hotel");
        assert_eq!(r.name.as_deref(), Some("Grand Hyatt Tokyo"));
        assert_eq!(r.start_date.as_deref(), Some("2026-05-01T15:00:00"));
    }

    #[test]
    fn extract_order() {
        let html = r#"<script type="application/ld+json">
        {
            "@type": "Order",
            "orderNumber": "W001234",
            "orderDate": "2026-03-01",
            "seller": {"@type": "Organization", "name": "Amazon"},
            "orderedItem": [
                {"@type": "Product", "name": "Rust Programming Book", "orderQuantity": 1, "price": "39.99"},
                {"@type": "Product", "name": "USB-C Cable", "orderQuantity": 2, "price": "9.99"}
            ],
            "totalPrice": "59.97",
            "priceCurrency": "USD"
        }
        </script>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.orders.len(), 1);
        let o = &data.orders[0];
        assert_eq!(o.order_number.as_deref(), Some("W001234"));
        assert_eq!(o.merchant.as_deref(), Some("Amazon"));
        assert_eq!(o.items.len(), 2);
        assert_eq!(o.items[0].name, "Rust Programming Book");
        assert_eq!(o.items[1].quantity, Some(2));
        assert_eq!(o.total.as_deref(), Some("59.97"));
        assert_eq!(o.currency.as_deref(), Some("USD"));
    }

    #[test]
    fn extract_event() {
        let html = r#"<script type="application/ld+json">
        {
            "@type": "Event",
            "name": "RustConf 2026",
            "startDate": "2026-09-10",
            "endDate": "2026-09-12",
            "location": {"@type": "Place", "name": "Portland Convention Center"},
            "url": "https://rustconf.com"
        }
        </script>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.events.len(), 1);
        let e = &data.events[0];
        assert_eq!(e.name, "RustConf 2026");
        assert_eq!(e.location.as_deref(), Some("Portland Convention Center"));
    }

    #[test]
    fn extract_potential_action() {
        let html = r#"<script type="application/ld+json">
        {
            "@type": "Order",
            "orderNumber": "123",
            "potentialAction": {
                "@type": "TrackAction",
                "name": "Track Package",
                "target": {"urlTemplate": "https://track.example.com/123"}
            }
        }
        </script>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.orders.len(), 1);
        assert_eq!(data.actions.len(), 1);
        assert_eq!(data.actions[0].kind, "track");
        assert_eq!(data.actions[0].url.as_deref(), Some("https://track.example.com/123"));
    }

    #[test]
    fn multiple_jsonld_blocks() {
        let html = r#"
        <script type="application/ld+json">{"@type":"Event","name":"Event A","startDate":"2026-01-01"}</script>
        <p>some text</p>
        <script type="application/ld+json">{"@type":"Event","name":"Event B","startDate":"2026-02-01"}</script>
        "#;

        let data = extract_structured_data(html);
        assert_eq!(data.events.len(), 2);
        assert_eq!(data.events[0].name, "Event A");
        assert_eq!(data.events[1].name, "Event B");
    }

    #[test]
    fn graph_array() {
        let html = r#"<script type="application/ld+json">
        {
            "@context": "http://schema.org",
            "@graph": [
                {"@type": "Event", "name": "Talk 1", "startDate": "2026-01-01"},
                {"@type": "Event", "name": "Talk 2", "startDate": "2026-01-02"}
            ]
        }
        </script>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.events.len(), 2);
    }

    #[test]
    fn no_jsonld() {
        let html = "<html><body><p>Just plain HTML</p></body></html>";
        let data = extract_structured_data(html);
        assert!(data.is_empty());
    }

    #[test]
    fn invalid_json() {
        let html = r#"<script type="application/ld+json">{not valid json}</script>"#;
        let data = extract_structured_data(html);
        assert!(data.is_empty());
    }

    #[test]
    fn restaurant_reservation() {
        let html = r#"<script type="application/ld+json">
        {
            "@type": "FoodEstablishmentReservation",
            "reservationId": "RES-456",
            "startTime": "2026-04-15T19:00:00",
            "reservationFor": {
                "@type": "FoodEstablishment",
                "name": "Sushi Dai",
                "address": {"streetAddress": "Tsukiji, Tokyo"}
            }
        }
        </script>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.reservations.len(), 1);
        assert_eq!(data.reservations[0].kind, "restaurant");
        assert_eq!(data.reservations[0].name.as_deref(), Some("Sushi Dai"));
    }

    #[test]
    fn rental_car_reservation() {
        let html = r#"<script type="application/ld+json">
        {
            "@type": "RentalCarReservation",
            "reservationId": "CAR-789",
            "pickupTime": "2026-06-01T10:00:00",
            "dropoffTime": "2026-06-05T10:00:00",
            "pickupLocation": {"@type": "Place", "name": "Narita Airport"},
            "provider": {"@type": "Organization", "name": "Toyota Rent a Car"},
            "reservationFor": {"@type": "RentalCar", "name": "Toyota Corolla"}
        }
        </script>"#;

        let data = extract_structured_data(html);
        assert_eq!(data.reservations.len(), 1);
        let r = &data.reservations[0];
        assert_eq!(r.kind, "rental_car");
        assert_eq!(r.location.as_deref(), Some("Narita Airport"));
        assert_eq!(r.provider.as_deref(), Some("Toyota Rent a Car"));
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn empty_html_returns_empty() {
        assert!(extract_structured_data("").is_empty());
    }

    #[test]
    fn jsonld_with_only_whitespace_inside_ignored() {
        let html = r#"<script type="application/ld+json">    </script>"#;
        assert!(extract_structured_data(html).is_empty());
    }

    #[test]
    fn ignores_non_jsonld_script_tags() {
        let html = r#"<script type="text/javascript">var x = 1;</script>"#;
        assert!(extract_structured_data(html).is_empty());
    }

    #[test]
    fn mixed_jsonld_and_html_content() {
        let html = r#"<html><body><p>Hi</p>
        <script type="application/ld+json">{"@type":"Order","orderNumber":"X","seller":{"name":"S"}}</script>
        <p>Bye</p></body></html>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.orders.len(), 1);
    }

    #[test]
    fn jsonld_uppercase_type_attribute() {
        // Some emails use TYPE= instead of type=. Our matcher lowercases
        // the search string, so this should still work.
        let html = r#"<script TYPE="APPLICATION/LD+JSON">{"@type":"Event","name":"E","startDate":"2026-01-01"}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.events.len(), 1);
    }

    #[test]
    fn rejects_malformed_jsonld() {
        let html = r#"<script type="application/ld+json">{not "valid": json,,,}</script>"#;
        assert!(extract_structured_data(html).is_empty());
    }

    #[test]
    fn event_without_name_still_extracted_with_empty_name() {
        let html = r#"<script type="application/ld+json">{"@type":"Event","startDate":"2026-09-10"}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.events.len(), 1);
        assert_eq!(data.events[0].name, "");
    }

    #[test]
    fn order_with_empty_items_list() {
        let html = r#"<script type="application/ld+json">{"@type":"Order","orderNumber":"X","orderedItem":[]}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.orders.len(), 1);
        assert!(data.orders[0].items.is_empty());
    }

    #[test]
    fn potential_action_unknown_type_skipped() {
        let html = r#"<script type="application/ld+json">{"@type":"Order","orderNumber":"X","potentialAction":{"@type":"WhatIsThisAction","name":"???"}}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.actions.len(), 0);
    }

    #[test]
    fn url_with_javascript_scheme_filtered_out() {
        let html = r#"<script type="application/ld+json">{"@type":"Event","name":"E","startDate":"2026-01-01","url":"javascript:alert(1)"}</script>"#;
        let data = extract_structured_data(html);
        assert!(data.events[0].url.is_none());
    }

    #[test]
    fn url_data_scheme_filtered_out() {
        let html = r#"<script type="application/ld+json">{"@type":"Event","name":"E","startDate":"2026-01-01","url":"data:text/html,evil"}</script>"#;
        let data = extract_structured_data(html);
        assert!(data.events[0].url.is_none());
    }

    #[test]
    fn url_http_preserved() {
        let html = r#"<script type="application/ld+json">{"@type":"Event","name":"E","startDate":"2026-01-01","url":"http://insecure.example.com"}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.events[0].url.as_deref(), Some("http://insecure.example.com"));
    }

    #[test]
    fn missing_optional_fields_are_none() {
        let html = r#"<script type="application/ld+json">{"@type":"FlightReservation"}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.reservations.len(), 1);
        let r = &data.reservations[0];
        assert!(r.reservation_id.is_none());
        assert!(r.flight_number.is_none());
        assert!(r.departure_airport.is_none());
    }

    #[test]
    fn order_status_strips_schema_url() {
        let html = r#"<script type="application/ld+json">{"@type":"Order","orderNumber":"X","orderStatus":"http://schema.org/OrderProcessing"}</script>"#;
        let data = extract_structured_data(html);
        assert_eq!(data.orders[0].status.as_deref(), Some("Processing"));
    }

    #[test]
    fn is_empty_returns_true_for_default() {
        let data = StructuredData::default();
        assert!(data.is_empty());
    }

    #[test]
    fn is_empty_false_when_anything_present() {
        let mut data = StructuredData::default();
        data.events.push(EventInfo {
            name: "x".into(),
            start_date: None,
            end_date: None,
            location: None,
            url: None,
        });
        assert!(!data.is_empty());
    }
}
