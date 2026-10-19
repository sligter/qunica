FROM node:24-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends python3 chromium xvfb openbox x11vnc xdotool scrot ca-certificates git curl \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g @playwright/mcp@0.0.83 opencode-ai@1.19.3
RUN groupadd --gid 10001 agent \
    && useradd --uid 10001 --gid 10001 --create-home --shell /bin/sh agent \
    && mkdir -p /home/agent/workspace \
    && chown agent:agent /home/agent/workspace
USER 10001:10001
ENV HOME=/home/agent
WORKDIR /home/agent/workspace
COPY docker/computer-service.mjs /opt/qunica/service.mjs
COPY docker/computer-start.sh /opt/qunica/start.sh
COPY backend-rs/crates/computer-supervisor/src/exec_guard.py /opt/qunica/exec_guard.py
CMD ["/bin/sh", "/opt/qunica/start.sh"]
