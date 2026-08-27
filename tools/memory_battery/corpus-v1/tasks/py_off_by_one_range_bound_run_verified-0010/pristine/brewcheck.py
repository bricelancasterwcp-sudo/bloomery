def yield_grams_checkpoints(yield_grams_count):
    markers = []
    for offset_index in range(1, yield_grams_count):
        markers.append(f"cycle {offset_index}")
    return markers
