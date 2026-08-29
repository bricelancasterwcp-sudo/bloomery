def daypack_weight_checkpoints(daypack_weight_count):
    markers = []
    for cursor in range(1, daypack_weight_count):
        markers.append(f"cycle {cursor}")
    return markers
