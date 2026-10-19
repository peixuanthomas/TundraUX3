management-problem-backend-unavailable = Service unavailable. Open details to check the connection and requirements.
management-problem-missing-dependency = A required tool is missing. Open details to see which one.
management-problem-unsupported-environment = This environment does not support the feature. Open details for requirements.
management-problem-failure-detail = Query or operation failed. Open details for the cause, then refresh to retry.
management-action-query-details = Details
management-systemd-not-pid1 = PID 1 is { $process }, not systemd. System service control is unavailable. Use a Linux environment running systemd as PID 1; authorization cannot resolve this limitation.
management-service-manager-query = Cannot connect to the { $scope } service manager: { $error }
management-required-tool-missing = Required system tool { $name } is not installed. Install the corresponding system package, then refresh.
management-tool-query-failed = { $name } exited with status { $code }.
    Standard error: { $error }
    Standard output: { $output }
management-requirements = Requirements and next step
management-firewall-unavailable = Firewall rules are unavailable. You can still view listening ports.
management-firewalld-inactive = UFW is not installed and firewalld is not running. Rule inspection and changes require UFW or a running firewalld. Manage the firewall with the system tools first, or switch to listening ports.
management-firewall-tools-missing = Neither UFW nor firewalld is installed. This page cannot inspect or change firewall rules. Set up a supported firewall with the system tools first, or switch to listening ports.
management-source-unavailable = { $source }: { $reason }
management-schedule-source = { $scope } timers
management-schedule-limitations = Task result and pause limitations
management-backups-empty = No recovery records yet. Open details to learn how records are created and what can be restored.
management-backups-permission = Recovery records require system permission. Choose Authorize viewing backups to read them.
management-backups-how-title = Create a recovery record
management-backups-how = Saving supported system configuration through TundraUX3 creates a recovery record automatically, before the change. Return to System Configuration, Network, SSH or another management page to make the intended change, then refresh this page. No manual whole-system backup is created here.
management-backups-scope-title = What can be restored
management-backups-scope = Records cover supported configuration and account or group changes, including network, firewall, SSH, mount, software-source, service-definition and scheduled-task settings. Restoration checks later changes and may require connection confirmation. Package installations, ordinary service start/stop, changes made outside TundraUX3 and whole-system snapshots are outside this scope.
