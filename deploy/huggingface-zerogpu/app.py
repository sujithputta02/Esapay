"""
ESA AI Brain inference service for private Hugging Face Spaces (ZeroGPU).

Exposes Gradio queue endpoint `generate`:
  - POST /gradio_api/call/generate      {"data": ["<prompt>"]} -> {"event_id": "..."}
  - GET  /gradio_api/call/generate/{id} (Server-Sent Events)

Validation, privacy redaction, policy enforcement, and financial authority
remain strictly inside the Rust ESAPay core (`esa-agents` OutputValidator).
"""
import os

MOCK_MODE = os.environ.get("ESA_HF_MOCK_MODE", "").strip().lower() in {"1", "true", "yes"}

MODEL_ID = "Qwen/Qwen2.5-0.5B-Instruct"
MODEL_REVISION = "7ae557604adf67be50417f59c2c2f167def9a775"
MAX_PROMPT_CHARS = int(os.environ.get("HF_AI_MAX_PROMPT_CHARS", "8192"))
MAX_NEW_TOKENS = int(os.environ.get("HF_AI_MAX_NEW_TOKENS", "256"))

SYSTEM_PROMPT = (
    "You are ESA AI Brain, a payment infrastructure diagnosis assistant. "
    "Analyze the provided telemetry and respond ONLY with a single valid JSON object "
    "containing keys: hypothesis, root_cause (one of HOT_PARTITION, CAPACITY_ISSUE, "
    "NODE_DEGRADATION, TRAFFIC_SPIKE, OTHER, UNCERTAIN), confidence (number in [0.0, 1.0]), "
    "evidence_refs (array of metric/condition names), and recommended_action "
    "(CREATE_REPLICA, SHIFT_ROUTE, or null). "
    "Never output executable code, shell commands, SQL, or payment execution instructions."
)

if MOCK_MODE:
    spaces = None
    torch = None
    transformers = None
    gradio = None
    tokenizer = None
    model = None
else:
    import gradio
    import spaces
    import torch
    import transformers

    tokenizer = transformers.AutoTokenizer.from_pretrained(
        MODEL_ID,
        revision=MODEL_REVISION,
    )
    model = transformers.AutoModelForCausalLM.from_pretrained(
        MODEL_ID,
        revision=MODEL_REVISION,
        torch_dtype=torch.float16,
        device_map="auto",
    )


def _gpu_decorator(duration: int = 60):
    if spaces is not None and hasattr(spaces, "GPU"):
        return spaces.GPU(duration=duration)
    return lambda fn: fn


@_gpu_decorator(duration=60)
def generate_text(prompt: str) -> str:
    """ZeroGPU inference function returning structured diagnosis text."""
    if not isinstance(prompt, str) or not prompt.strip():
        raise ValueError("Prompt must be a non-empty string.")
    if len(prompt) > MAX_PROMPT_CHARS:
        raise ValueError(f"Prompt exceeds maximum length of {MAX_PROMPT_CHARS} characters.")
    if MOCK_MODE:
        raise RuntimeError("Inference disabled in ESA_HF_MOCK_MODE.")

    messages = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": prompt.strip()},
    ]
    text = tokenizer.apply_chat_template(
        messages,
        tokenize=False,
        add_generation_prompt=True,
    )
    inputs = tokenizer([text], return_tensors="pt").to(model.device)

    with torch.inference_mode():
        outputs = model.generate(
            **inputs,
            max_new_tokens=MAX_NEW_TOKENS,
            do_sample=False,
            pad_token_id=tokenizer.eos_token_id,
        )

    response = tokenizer.decode(
        outputs[0][inputs.input_ids.shape[1]:],
        skip_special_tokens=True,
    )
    return response.strip()


if gradio is not None:
    with gradio.Blocks(title="ESA AI Brain (ZeroGPU)") as demo:
        gradio.Markdown("# ESA AI Brain (Experimental ZeroGPU Inference)")
        gradio.Markdown(
            f"Model: `{MODEL_ID}` (`{MODEL_REVISION[:12]}`) — "
            "Private Gradio queue endpoint `generate`."
        )

        with gradio.Row():
            with gradio.Column(scale=1):
                prompt_input = gradio.Textbox(label="Sanitized Telemetry Prompt", lines=6)
                submit_btn = gradio.Button("Run Inference", variant="primary")
            with gradio.Column(scale=1):
                output_box = gradio.Textbox(label="Structured Diagnosis Output", lines=8)

        submit_btn.click(
            fn=generate_text,
            inputs=[prompt_input],
            outputs=[output_box],
            api_name="generate",
        )

    if __name__ == "__main__":
        demo.queue().launch(server_name="0.0.0.0", server_port=7860)
