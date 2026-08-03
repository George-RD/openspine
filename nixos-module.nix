{ self }:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.openspine;
in
{
  options.services.openspine = {
    enable = lib.mkEnableOption "the OpenSpine kernel service";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.default;
      description = "OpenSpine package providing the kernel and shell binaries.";
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      default = "/etc/openspine/openspine.yaml";
      description = "Path to the OpenSpine YAML configuration file.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.path;
      default = "/etc/openspine/openspine.env";
      description = "External EnvironmentFile containing OpenSpine secrets.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/openspine";
      description = "Persistent working directory for OpenSpine data.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "openspine";
      description = "Dedicated system user used by the OpenSpine service.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "openspine";
      description = "Primary group for the dedicated OpenSpine service user.";
    };

    docker = {
      enable = lib.mkEnableOption "Docker access for OpenSpine";

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.docker;
        description = "Docker package to enable when OpenSpine Docker access is enabled.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    virtualisation.docker = lib.mkIf cfg.docker.enable {
      enable = true;
      package = cfg.docker.package;
    };

    users.groups.${cfg.group} = { };
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      extraGroups = lib.optional cfg.docker.enable "docker";
    };

    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0750 ${cfg.user} ${cfg.group} -"
    ];

    systemd.services.openspine = {
      description = "OpenSpine kernel";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ]
        ++ lib.optional cfg.docker.enable "docker.service";

      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.dataDir;
        ExecStart = "${cfg.package}/bin/openspine --config ${cfg.configFile}";
        EnvironmentFile = cfg.environmentFile;
        Restart = "on-failure";
        RestartSec = "5s";
      };
    };
  };
}
