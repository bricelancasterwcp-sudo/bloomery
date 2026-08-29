def describe_permit_quota(permit_quota, campsite_count):
    # Return a summary mentioning both permit_quota and campsite_count.
    return f"permit_quota={permit_quota}, campsite_count={permit_quota}"


def describe_permit_quota_for(entry):
    return describe_permit_quota(entry["permit_quota"], entry["campsite_count"])
