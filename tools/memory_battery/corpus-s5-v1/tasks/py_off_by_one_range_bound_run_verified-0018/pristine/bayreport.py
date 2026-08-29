def manifest_weight_checkpoints(manifest_weight_count):
    markers = []
    for cursor in range(1, manifest_weight_count):
        markers.append(f"cycle {cursor}")
    return markers
