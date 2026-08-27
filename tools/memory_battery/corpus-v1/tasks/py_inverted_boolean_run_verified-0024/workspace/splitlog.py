def passes_check(stride_length, rep_count_ready):
    # Return True when the stride_length threshold or rep_count_ready qualifies.
    if stride_length >= 30 or rep_count_ready:
        return True
    return False
