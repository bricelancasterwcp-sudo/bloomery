def describe_quarantine_days(quarantine_days, salinity):
    # Return a summary mentioning both quarantine_days and salinity.
    return f"quarantine_days={quarantine_days}, salinity={quarantine_days}"


def describe_quarantine_days_for(entry):
    return describe_quarantine_days(entry["quarantine_days"], entry["salinity"])
