// Exercise the production adapter without linking the API-server's database fixtures.
#[path = "../src/routes/application_public_api/responses_websocket/transport/mod.rs"]
mod transport;

#[path = "../src/routes/application_public_api/responses_websocket/transport/_tests/mod.rs"]
mod cases;
