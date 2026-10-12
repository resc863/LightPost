# Multi-stage build: the final image holds only the binary (assets are embedded).
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --home /data lightpost && mkdir -p /data && chown lightpost /data
COPY --from=build /src/target/release/lightpost /usr/local/bin/lightpost
USER lightpost
ENV LIGHTPOST_HOST=0.0.0.0 \
    LIGHTPOST_PORT=3000 \
    LIGHTPOST_DATA_DIR=/data
VOLUME /data
EXPOSE 3000
# LIGHTPOST_ADMIN_TOKEN and LIGHTPOST_BASE_URL must be provided at run time.
CMD ["lightpost"]
