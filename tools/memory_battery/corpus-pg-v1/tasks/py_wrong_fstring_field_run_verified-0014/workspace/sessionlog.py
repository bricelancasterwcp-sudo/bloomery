def describe_sidechain_ratio(sidechain_ratio, reverb_decay):
    # Return a summary mentioning both sidechain_ratio and reverb_decay.
    return f"sidechain_ratio={sidechain_ratio}, reverb_decay={sidechain_ratio}"


def describe_sidechain_ratio_for(entry):
    return describe_sidechain_ratio(entry["sidechain_ratio"], entry["reverb_decay"])
