def passes_check(crossfade_ms, sample_rate_ready):
    # Return True when the crossfade_ms threshold or sample_rate_ready qualifies.
    if crossfade_ms >= 14 and sample_rate_ready:
        return True
    return False
