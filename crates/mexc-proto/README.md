# mexc-proto

Protobuf-generated Rust code for MEXC exchange API.

This crate contains the generated Rust structs and enums from MEXC's protobuf definitions, specifically for the aggregated deals API.

## Generated Types

- `PublicAggreDealsV3Api`: Contains aggregated deal information
- `PublicAggreDealsV3ApiItem`: Individual deal data with price, quantity, trade type, and timestamp
- `PushDataV3ApiWrapper`: WebSocket message wrapper containing the deal data

## Usage

```rust
use mexc_proto::{PushDataV3ApiWrapper, PublicAggreDealsV3Api};

// Decode protobuf message from WebSocket
let message: PushDataV3ApiWrapper = prost::Message::decode(&data)?;
```

## Build Dependencies

This crate requires `prost-build` to generate the code from `.proto` files. The generated code is committed to the repository to avoid requiring protobuf compilation during normal builds.
