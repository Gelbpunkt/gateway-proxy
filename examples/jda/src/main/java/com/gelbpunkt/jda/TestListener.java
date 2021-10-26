package com.gelbpunkt.jda;

import lombok.extern.slf4j.Slf4j;
import net.dv8tion.jda.api.events.StatusChangeEvent;
import net.dv8tion.jda.api.events.message.guild.GuildMessageReceivedEvent;
import net.dv8tion.jda.api.hooks.ListenerAdapter;
import net.dv8tion.jda.api.sharding.ShardManager;
import org.jetbrains.annotations.NotNull;
import org.springframework.stereotype.Component;

@Slf4j
@Component
public class TestListener extends ListenerAdapter {

    public TestListener(ShardManager api) {
        api.addEventListener(this);
    }

    @Override
    public void onStatusChange(@NotNull StatusChangeEvent event) {
        log.info("Status changed on shard {} to {}", event.getJDA().getShardInfo().getShardId(), event.getNewStatus().name());
    }

    @Override
    public void onGuildMessageReceived(@NotNull GuildMessageReceivedEvent event) {
        if (event.getGuild().getIdLong() != 213044545825406976L) {
            return;
        }
        log.info("Message received: " + event.getMessage());
    }

}
