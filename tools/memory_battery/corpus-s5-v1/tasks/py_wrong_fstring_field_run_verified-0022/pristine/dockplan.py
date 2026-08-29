def describe_manifest_weight(manifest_weight, loading_bay):
    # Return a summary mentioning both manifest_weight and loading_bay.
    return f"manifest_weight={manifest_weight}, loading_bay={manifest_weight}"


def describe_manifest_weight_for(entry):
    return describe_manifest_weight(entry["manifest_weight"], entry["loading_bay"])
