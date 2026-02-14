FROM public.ecr.aws/docker/library/rust:1.79 as builder
WORKDIR /app
RUN apt-get update && apt-get install -y protobuf-compiler

COPY proto proto
COPY submitter submitter

WORKDIR /app/submitter
RUN cargo build --release --bin submitter

FROM public.ecr.aws/docker/library/debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates openssl gettext-base && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/submitter/target/release/submitter /usr/local/bin/submitter

CMD ["submitter"]
