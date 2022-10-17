FROM rust:1.64.0 AS build

RUN cargo search serde # pre-warm crates cache

WORKDIR /app
COPY Cargo.* ./
COPY src ./src/

RUN cargo build --bin override-operator

FROM gcr.io/distroless/cc
COPY --from=build /app/target/debug/override-operator /
  
CMD ["./override-operator"]
