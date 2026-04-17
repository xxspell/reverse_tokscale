FROM rust:1.86 AS build
WORKDIR /app
COPY . .
RUN cargo build --release

FROM node:20-bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/tokscale-activity-emulator /usr/local/bin/tokscale-activity-emulator
RUN npm install -g tokscale@latest
CMD ["tokscale-activity-emulator"]
