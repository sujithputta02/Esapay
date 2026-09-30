FROM ollama/ollama:latest

# Create standard Hugging Face non-root user (UID 1000)
RUN useradd -m -u 1000 user
ENV HOME=/home/user \
    OLLAMA_MODELS=/home/user/.ollama/models \
    OLLAMA_HOST=0.0.0.0:7860 \
    OLLAMA_KEEP_ALIVE=-1

RUN mkdir -p /home/user/.ollama/models && chown -R 1000:1000 /home/user

# Switch to non-root user
USER 1000
WORKDIR /home/user

# Pre-bake model during build for instantaneous boot
RUN /bin/ollama serve & \
    SERVER_PID=$! && \
    until /bin/ollama list > /dev/null 2>&1; do sleep 1; done && \
    /bin/ollama pull qwen2.5:0.5b && \
    kill -TERM $SERVER_PID && \
    wait $SERVER_PID || true

# Copy entrypoint script
COPY --chown=1000:1000 entrypoint.sh /home/user/entrypoint.sh
RUN chmod +x /home/user/entrypoint.sh

EXPOSE 7860

ENTRYPOINT ["/home/user/entrypoint.sh"]
