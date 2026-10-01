$devs = Get-PnpDevice -Class MEDIA | Where-Object { $_.FriendlyName -like '*Intel*' }
foreach ($d in $devs) {
    Write-Output "Device: $($d.FriendlyName) | Status: $($d.Status)"
    Write-Output "InstanceId: $($d.InstanceId)"
    try {
        Disable-PnpDevice -InstanceId $d.InstanceId -Confirm:$false -ErrorAction Stop
        Write-Output "  Disabled OK"
        Start-Sleep -Seconds 2
        Enable-PnpDevice -InstanceId $d.InstanceId -Confirm:$false -ErrorAction Stop
        Write-Output "  Enabled OK (restarted)"
    } catch {
        Write-Output "  Failed: $($_.Exception.Message)"
    }
    Write-Output ""
}
