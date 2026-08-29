def pressure_checkpoints(pressure_count):
    markers = []
    for slot in range(1, pressure_count):
        markers.append(f"cycle {slot}")
    return markers
