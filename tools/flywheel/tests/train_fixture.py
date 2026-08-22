"""A mini tokenizer that can spell `</action>` as one token, for the
trainer tests (the prune fixture's tokenizer cannot — WordLevel over t0..tN
with a punctuation-splitting pre-tokenizer)."""
from tokenizers import Tokenizer, models, pre_tokenizers, decoders
from transformers import PreTrainedTokenizerFast

from .prune_fixture import VOCAB_SIZE  # the mini model's vocab size

ACTION_END = "</action>"


def build_action_tokenizer() -> PreTrainedTokenizerFast:
    vocab = {f"t{i}": i for i in range(VOCAB_SIZE - 2)}
    vocab[ACTION_END] = VOCAB_SIZE - 2
    vocab["<unk>"] = VOCAB_SIZE - 1
    backend = Tokenizer(models.WordLevel(vocab, unk_token="<unk>"))
    backend.pre_tokenizer = pre_tokenizers.WhitespaceSplit()
    backend.decoder = decoders.WordPiece(prefix="", cleanup=False)
    return PreTrainedTokenizerFast(tokenizer_object=backend, unk_token="<unk>")


def tiny_corpus(n: int = 6):
    """Rows in the corpus shape: prompt, completion ending at </action>, meta.task_id."""
    return [{"prompt": f"t1 t2 t{i} ", "completion": f"t4 t5 t{i}\n{ACTION_END}", "meta": {"task_id": f"task-{i}"}}
            for i in range(n)]
