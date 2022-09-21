FROM rust:1.63 AS builder
WORKDIR /app
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --release

FROM debian:bullseye AS runtime
RUN apt update && apt upgrade -y && apt install -y ca-certificates
COPY --from=builder /app/target/release/filestore /usr/local/bin
ENTRYPOINT [ "/usr/local/bin/filestore" ]
