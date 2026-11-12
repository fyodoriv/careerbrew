#!/usr/bin/env bash
# US-20: bring LinkedIn in without scraping: my data export and my job-alert emails.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
python3 - "$SCRATCH/export.zip" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("Positions.csv", "Company Name,Title,Description,Location,Started On,Finished On\nOldCo,Software Engineer,,Example City,Jan 2020,Dec 2023\n")
    z.writestr("Skills.csv", "Name\nKubernetes\n")
    z.writestr("Jobs/Saved Jobs.csv", "Saved Date,Job Url,Job Title,Company Name\n10/1/26,https://www.linkedin.com/jobs/view/1,Senior Software Engineer,BirchCo\n")
    z.writestr("Jobs/Job Applications_1.csv", "Application Date,Contact Email,Contact Phone Number,Company Name,Job Title,Job Url,Resume Name,Question And Answers\n9/1/26,,,ElmCo,Staff Engineer,https://x,cv.pdf,\n")
PY
expect "$(cb jobs linkedin-import "$SCRATCH/export.zip")" "1 saved job(s), 1 application(s) logged" "export imported"
expect "$(cb jobs list)" "BirchCo" "saved job appears in the list"
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_FM="e2e-token"
echo "accounts: [{name: personal, kind: jmap, token: env:CB_FM}]" > "$CAREERBREW_HOME/mail.yaml"
expect "$(cb mail scan)" "LinkedIn alerts: 1 new posting" "alert email became a posting"
expect "$(cb jobs list)" "ZzPlatformCo" "alert posting listed"
expect "$(cb jobs linkedin-import "$SCRATCH/missing.zip" 2>&1 || true)" "error: " "error case: missing archive"
echo "ok US-20 linkedin import"
