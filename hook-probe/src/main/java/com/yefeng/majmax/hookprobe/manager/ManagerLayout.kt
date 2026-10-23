package com.yefeng.majmax.hookprobe.manager

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardColors
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.ProvideTextStyle
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

internal object ManagerLayout {
    val pagePadding = 16.dp
    val cardPadding = 16.dp
    val sectionGap = 16.dp
    val contentGap = 12.dp
    val actionGap = 8.dp
    val maxContentWidth = 640.dp
    val cardShape = RoundedCornerShape(16.dp)
    val buttonShape = RoundedCornerShape(12.dp)
}

/** Shared page width keeps cards aligned on phones and readable in landscape. */
@Composable
internal fun ManagerPage(padding: PaddingValues, content: @Composable ColumnScope.() -> Unit) {
    ManagerContent(padding) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ManagerLayout.pagePadding),
            verticalArrangement = Arrangement.spacedBy(ManagerLayout.sectionGap),
            content = content,
        )
    }
}

@Composable
internal fun ManagerContent(padding: PaddingValues, content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
        Box(Modifier.widthIn(max = ManagerLayout.maxContentWidth).fillMaxSize()) { content() }
    }
}

@Composable
internal fun ManagerCard(
    modifier: Modifier = Modifier,
    colors: CardColors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
    onClick: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val body: @Composable () -> Unit = {
        Column(Modifier.fillMaxWidth().padding(ManagerLayout.cardPadding),
            verticalArrangement = Arrangement.spacedBy(ManagerLayout.contentGap), content = content)
    }
    if (onClick == null) {
        Card(modifier.fillMaxWidth(), shape = ManagerLayout.cardShape, colors = colors) { body() }
    } else {
        Card(onClick = onClick, modifier = modifier.fillMaxWidth(), shape = ManagerLayout.cardShape,
            colors = colors) { body() }
    }
}

/** Equal-width, equal-height actions; stack when width or font scaling needs it. */
@Composable
internal fun ManagerActions(
    first: @Composable (Modifier) -> Unit,
    second: @Composable (Modifier) -> Unit,
) {
    val fontScale = LocalDensity.current.fontScale.coerceAtLeast(1f)
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        if (maxWidth < 296.dp * fontScale) {
            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(ManagerLayout.actionGap)) {
                first(Modifier.fillMaxWidth())
                second(Modifier.fillMaxWidth())
            }
        } else {
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min),
                horizontalArrangement = Arrangement.spacedBy(ManagerLayout.actionGap)) {
                first(Modifier.weight(1f).fillMaxHeight())
                second(Modifier.weight(1f).fillMaxHeight())
            }
        }
    }
}

@Composable
internal fun ManagerButton(
    onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    Button(onClick = onClick, modifier = modifier.fillMaxWidth().heightIn(min = 48.dp), enabled = enabled,
        shape = ManagerLayout.buttonShape, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp)) {
        ProvideTextStyle(MaterialTheme.typography.labelLarge.copy(textAlign = TextAlign.Center)) { content() }
    }
}

@Composable
internal fun ManagerOutlinedButton(
    onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    OutlinedButton(onClick = onClick, modifier = modifier.fillMaxWidth().heightIn(min = 48.dp), enabled = enabled,
        shape = ManagerLayout.buttonShape, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp)) {
        ProvideTextStyle(MaterialTheme.typography.labelLarge.copy(textAlign = TextAlign.Center)) { content() }
    }
}

@Composable
internal fun ManagerTextButton(
    onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    TextButton(onClick = onClick, modifier = modifier.fillMaxWidth().heightIn(min = 48.dp), enabled = enabled,
        shape = ManagerLayout.buttonShape, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp)) {
        ProvideTextStyle(MaterialTheme.typography.labelLarge.copy(textAlign = TextAlign.Center)) { content() }
    }
}
