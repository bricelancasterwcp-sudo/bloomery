"""Miniature `qwen3_5_moe` fixtures shared by the `test_prune_*` modules.

Not named `test_*` on purpose: unittest discovery must not pick it up.
Importing it requires torch + transformers, so every test module that uses
it raises `unittest.SkipTest` at import time when those are absent (see
`tools/flywheel/prune/README` notes in the task-A report).

The model built here is a real `Qwen3_5MoeForCausalLM` — the text-only
class the spike resolved to — with the real hybrid `layer_types` pattern
(linear_attention / full_attention) so the pure-torch Gated-DeltaNet
fallback is exercised without `fla` or `causal_conv1d`.
"""

import torch
from tokenizers import Tokenizer, models, pre_tokenizers
from transformers import PreTrainedTokenizerFast, Qwen3_5MoeForCausalLM
from transformers.models.qwen3_5_moe import Qwen3_5MoeTextConfig

# Mini geometry. Four layers with one full-attention layer (index 3, i.e.
# the real `full_attention_interval=4` pattern: (i+1) % 4 == 0) and three
# Gated-DeltaNet layers.
VOCAB_SIZE = 64
HIDDEN_SIZE = 64
NUM_LAYERS = 4
NUM_EXPERTS = 8
TOP_K = 2


def mini_config(**overrides) -> Qwen3_5MoeTextConfig:
    """A miniature text config that keeps every structural feature we prune."""
    kwargs = dict(
        vocab_size=VOCAB_SIZE,
        hidden_size=HIDDEN_SIZE,
        num_hidden_layers=NUM_LAYERS,
        num_attention_heads=4,
        num_key_value_heads=2,
        head_dim=16,
        max_position_embeddings=128,
        moe_intermediate_size=32,
        shared_expert_intermediate_size=32,
        num_experts=NUM_EXPERTS,
        num_experts_per_tok=TOP_K,
        linear_conv_kernel_dim=4,
        linear_key_head_dim=16,
        linear_value_head_dim=16,
        linear_num_key_heads=2,
        linear_num_value_heads=4,
        tie_word_embeddings=False,
        rms_norm_eps=1e-6,
    )
    kwargs.update(overrides)
    cfg = Qwen3_5MoeTextConfig(**kwargs)
    # `full_attention_interval=4` on 4 layers -> the last layer is dense
    # attention, the rest are linear. Assert it so a transformers change
    # that silently flips the pattern fails loudly here.
    assert cfg.layer_types == [
        "linear_attention",
        "linear_attention",
        "linear_attention",
        "full_attention",
    ], cfg.layer_types
    return cfg


def build_mini_model(seed: int = 20260822, pin_eager: bool = True,
                     **overrides) -> Qwen3_5MoeForCausalLM:
    """Randomly-initialised miniature model in eval mode.

    `pin_eager=True` (the default) pins the eager expert kernel so tests
    compare like with like. `pin_eager=False` leaves whatever transformers
    picks — `grouped_mm` on this box — which is what the observer's kernel
    swap has to be measured against.
    """
    torch.manual_seed(seed)
    model = Qwen3_5MoeForCausalLM(mini_config(**overrides))
    model.eval()
    if pin_eager:
        model.set_experts_implementation("eager")
    return model


def build_mini_dense_model(seed: int = 20260822):
    """A tiny NON-MoE causal LM, for the architecture-rejection test."""
    from transformers import Qwen3Config, Qwen3ForCausalLM

    torch.manual_seed(seed)
    config = Qwen3Config(
        vocab_size=VOCAB_SIZE, hidden_size=32, num_hidden_layers=2,
        num_attention_heads=2, num_key_value_heads=1, head_dim=16,
        intermediate_size=32, max_position_embeddings=64,
        tie_word_embeddings=False)
    return Qwen3ForCausalLM(config).eval()


def mini_input_ids(seed: int = 7, batch: int = 1, seq: int = 12) -> torch.Tensor:
    gen = torch.Generator().manual_seed(seed)
    return torch.randint(0, VOCAB_SIZE, (batch, seq), generator=gen)


def build_mini_tokenizer() -> PreTrainedTokenizerFast:
    """A whitespace word-level tokenizer over `t0..t{VOCAB_SIZE-2}`.

    Built in-process so the CLI smoke test needs no network and no
    downloaded tokenizer.
    """
    vocab = {f"t{i}": i for i in range(VOCAB_SIZE - 1)}
    vocab["<unk>"] = VOCAB_SIZE - 1
    backend = Tokenizer(models.WordLevel(vocab, unk_token="<unk>"))
    backend.pre_tokenizer = pre_tokenizers.Whitespace()
    return PreTrainedTokenizerFast(tokenizer_object=backend, unk_token="<unk>")


def mini_calibration_texts(n: int = 6, seed: int = 3) -> list[str]:
    """Deterministic whitespace text the mini tokenizer can encode."""
    import random

    rng = random.Random(seed)
    return [
        " ".join(f"t{rng.randrange(VOCAB_SIZE - 1)}" for _ in range(16))
        for _ in range(n)
    ]
