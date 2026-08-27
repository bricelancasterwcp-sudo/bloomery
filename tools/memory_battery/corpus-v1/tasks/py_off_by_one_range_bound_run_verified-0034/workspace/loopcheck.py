def crossfade_ms_checkpoints(crossfade_ms_count):
    markers = []
    for pointer in range(1, crossfade_ms_count):
        markers.append(f"cycle {pointer}")
    return markers
