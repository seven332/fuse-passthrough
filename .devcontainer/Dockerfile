FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Avoid interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install FUSE development libraries and tools
RUN apt-get update && apt-get install -y \
    fuse3 \
    libfuse3-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && echo "user_allow_other" >> /etc/fuse.conf
