package com.gelbpunkt.jda;

import lombok.extern.slf4j.Slf4j;
import net.bytebuddy.ByteBuddy;
import net.bytebuddy.agent.ByteBuddyAgent;
import net.bytebuddy.dynamic.loading.ClassReloadingStrategy;
import net.bytebuddy.implementation.StubMethod;
import net.dv8tion.jda.api.requests.GatewayIntent;
import net.dv8tion.jda.api.sharding.DefaultShardManagerBuilder;
import net.dv8tion.jda.api.sharding.ShardManager;
import net.dv8tion.jda.api.utils.Compression;
import net.dv8tion.jda.internal.handle.ReadyHandler;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.context.annotation.Bean;

import javax.security.auth.login.LoginException;

import static net.bytebuddy.matcher.ElementMatchers.named;

@Slf4j
@SpringBootApplication
public class Application {

    public static void main(String[] args) {
        // Intercept handleReady and No-Op it because it's not needed since bots became a thing.
        ByteBuddyAgent.install();
        new ByteBuddy()
                .redefine(ReadyHandler.class)
                .method(named("handleReady"))
                .intercept(StubMethod.INSTANCE)
                .make()
                .load(Application.class.getClassLoader(), ClassReloadingStrategy.fromInstalledAgent());
        SpringApplication.run(Application.class, args);
    }

    @Bean
    public ShardManager jda(AppConfig config) {
        try {
            ShardManager manager = DefaultShardManagerBuilder
                    .createDefault(config.getToken())
                    .setSessionController(new GatewayController(config))
                    .enableIntents(GatewayIntent.GUILD_MEMBERS)
                    .setCompression(Compression.NONE)
                    .setShardsTotal(2)
                    .setShards(0, 1)
                    .build();
            log.info("Intents: " + GatewayIntent.getRaw(manager.getGatewayIntents()));
            return manager;
        } catch (LoginException ex) {
            throw new IllegalStateException("Failed to start JDA", ex);
        }
    }

}
