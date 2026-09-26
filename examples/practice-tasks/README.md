# Try useful work, then check the result

These three tiny Python tasks let you practice with Nosis or another coding assistant.
They need Python 3.9 or later only to run the checks. Nosis itself does not need Python.
They are a starting point for comparison, not evidence that one assistant is better.

**Portable download users:** the release ZIP does not contain these practice files.
From this source tree, open [starter.py](starter.py) and [check.py](check.py). On
GitHub, use **Download raw file** on each page; in a local checkout, copy the files.
Until this source is published, these files are available only in the local checkout.
Save them in a practice-materials folder, then follow the steps below.

Use a new scratch folder. Copy `starter.py` there as `work.py`. Keep `check.py` outside
the folder the assistant edits. The starter contains deliberate mistakes; a failing
check before the task is expected. Never point this exercise at valuable project files.

Give the assistant one task below. Select a model explicitly. Inspect its resulting
Python before running the checks: those checks execute the code and are not a sandbox.
For each new attempt, make a fresh copy of the original starter.

## Task 1: fix a calculation

Paste this task into your assistant:

> Fix only delivery_fee in work.py. Inputs are an integer subtotal in cents and an
> optional express boolean. Standard delivery costs 499 cents below 5000 cents and
> is free at or above 5000. Express always adds 799 cents, even when standard delivery
> is free. Negative subtotals must raise ValueError. Keep the signature and other
> functions unchanged. Explain the cases you fixed. Do not claim tests ran unless
> you actually ran them.

From the directory containing `check.py`, run this command after replacing the path:

```powershell
python -B check.py C:\path\to\scratch\work.py delivery
```

Success: four checks pass and only the requested function changed.

## Task 2: preserve user data while cleaning it

> Fix only unique_names in work.py. Given a list of strings, trim surrounding
> whitespace, skip empty names, and remove duplicates using Unicode casefold.
> Preserve the spelling and order of each first occurrence. Never modify the input
> list. Keep the signature and other functions unchanged. Explain your change.

```powershell
python -B check.py C:\path\to\scratch\work.py names
```

Success: four checks pass and only the requested function changed.

## Task 3: add a small feature

> Implement only parse_quantities in work.py. Read a list of strings containing
> item,quantity. Ignore lines that are empty or contain only whitespace. Trim both fields. Labels are nonempty and
> case-sensitive. Quantities are nonnegative whole numbers. Sum repeated labels
> into a dictionary. Malformed lines, extra commas and invalid quantities must raise
> ValueError. Keep the signature and other functions unchanged. Explain your change.

```powershell
python -B check.py C:\path\to\scratch\work.py quantities
```

Success: four checks pass and only the requested function changed.

## First, ask without allowing edits

**Unreleased source feature:** `nh run --read-only` permits local file reading and
searching, but gives the model no editing, shell or remote-tool capability. It still
sends allowed content to your selected provider and writes normal local receipts.
Normal provider charges apply. It does not run the Python checks.

After `nh init` in your scratch folder, you can inspect the first task with:

```powershell
nh run "Read work.py and explain the delivery_fee bugs. Propose a fix." --read-only --model deepseek-v4-flash
```

For an actual edit, start a separate ordinary run with the complete task above.
Check the file afterward. A convincing explanation is not proof of correct code.

## Compare honestly

Use the same starting files, task, exact API model, reasoning setting and attempt
limit for every assistant. Record default differences instead of concealing them.
Save this small table for each attempt:

| Assistant/version | Model/settings | Task | Checks passed | Unrequested edits | Elapsed seconds | Reported cost | Human interventions |
| --- | --- | --- | --- | --- | --- | --- | --- |
| | | | | | | | |

Unknown cost stays unknown. Record failed attempts and their cost too. Count a task
as complete only after its checks pass and a person confirms the requested scope.
Compare total spend per correctly completed task, not token prices alone. Repeat
attempts and include realistic projects before drawing purchasing or marketing
conclusions. These visible, small tests are practice checks, not a hidden benchmark.
