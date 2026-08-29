def greenhouse_temp_checkpoints(greenhouse_temp_count):
    markers = []
    for pointer in range(1, greenhouse_temp_count):
        markers.append(f"cycle {pointer}")
    return markers
