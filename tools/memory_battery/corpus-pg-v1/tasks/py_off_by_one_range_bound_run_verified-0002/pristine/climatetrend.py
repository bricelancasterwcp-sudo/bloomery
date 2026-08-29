def rainfall_checkpoints(rainfall_count):
    markers = []
    for pointer in range(1, rainfall_count):
        markers.append(f"cycle {pointer}")
    return markers
