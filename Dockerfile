# Builder stage
FROM rust:1.76-bookworm as builder

WORKDIR /usr/src/app
COPY . .

# Build the application
RUN cargo install --path .

# Runtime stage
FROM debian:bookworm-slim

# Install OpenSSL (required for ethers-rs/reqwest) and ca-certificates
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/submitter-rs /usr/local/bin/submitter-rs

# Create a non-root user
RUN useradd -m -u 1000 -U submitter
USER submitter

ENTRYPOINT ["submitter-rs"]
