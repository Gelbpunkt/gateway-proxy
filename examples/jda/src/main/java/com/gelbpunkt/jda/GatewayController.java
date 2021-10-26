package com.gelbpunkt.jda;

import net.dv8tion.jda.api.AccountType;
import net.dv8tion.jda.api.JDA;
import net.dv8tion.jda.api.exceptions.AccountTypeException;
import net.dv8tion.jda.api.utils.SessionControllerAdapter;
import org.jetbrains.annotations.NotNull;

public class GatewayController extends SessionControllerAdapter {

    private final AppConfig config;

    public GatewayController(AppConfig config) {
        this.config = config;
    }

    @NotNull
    @Override
    public String getGateway(@NotNull JDA api) {
        return config.getGateway();
    }

    @NotNull
    @Override
    public ShardedGateway getShardedGateway(@NotNull JDA api) {
        AccountTypeException.check(api.getAccountType(), AccountType.BOT);
        log.info("Starting against " + config.getGateway());
        return new ShardedGateway(config.getGateway(), config.getShards(), config.getShards());
    }

}
